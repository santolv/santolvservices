use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tiny_http::{Header, Method, Response, Server};
use urlencoding::decode;
use walkdir::WalkDir;

mod accounts;
mod ai;
mod craft;
mod server;

const INDEX_HTML: &str = include_str!("index.html");
const HUB_HTML: &str = include_str!("hub.html");
const HUB_JS: &str = include_str!("ui/hub.js");
const REFERENCES_JS: &str = include_str!("ui/references.js");
const CRAFT_JS: &str = include_str!("ui/craft-editor.js");
const CRAFT_MERGE_JS: &str = include_str!("ui/craft-merge.bundle.js");
const ACCOUNT_JS: &str = include_str!("ui/accounts.js");
const LOGIN_HTML: &str = include_str!("login.html");
const CHAT_JS: &str = include_str!("ui/chat.js");
const CHAT_CSS: &str = include_str!("ui/chat.css");
const STAGESHOW_CSS: &str = include_str!("ui/stageshow.css");
const HUB_CSS: &str = include_str!("ui/hub.css");
const APP_CSS: &str = include_str!("ui/app.css");
const NOTION_CSS: &str = include_str!("ui/notion.css");
const APP_JS: &str = include_str!("ui/bundle.js");

fn ui_asset(relative: &str, bundled: &str) -> String {
    // Debug builds pick up rebuilt assets on reload; release builds stay self-contained.
    if cfg!(debug_assertions) {
        if let Ok(asset) = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join(relative),
        ) {
            return asset;
        }
    }
    bundled.to_string()
}

#[derive(Serialize, Deserialize)]
struct FileEntry {
    path: String,
    name: String,
    is_dir: bool,
}

#[derive(Deserialize)]
struct WriteReq {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct FolderReq {
    path: String,
}

#[derive(Deserialize)]
struct RenameReq {
    from: String,
    to: String,
}

#[derive(Serialize)]
struct SearchHit {
    path: String,
    snippet: String,
}

#[derive(Serialize)]
struct OutlineItem {
    level: u8,
    text: String,
    line: usize,
}

#[derive(Serialize)]
struct GraphNode {
    id: String,
    path: String,
    title: String,
}

#[derive(Serialize)]
struct GraphEdge {
    source: String,
    target: String,
}

#[derive(Serialize)]
struct Graph {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

fn vault_root() -> PathBuf {
    if let Some(root) = std::env::var_os("SUPER_CEREBRO_VAULT") {
        let root = PathBuf::from(root);
        let _ = fs::create_dir_all(&root);
        return root;
    }
    let mut p = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    p.push("vault");
    if !p.exists() {
        let _ = fs::create_dir_all(&p);
    }
    p
}

fn safe_path(rel: &str) -> Option<PathBuf> {
    safe_path_at(&vault_root(), rel)
}

fn safe_path_at(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.replace('\\', "/");
    if rel.is_empty() {
        return None;
    }
    let mut full = root.to_path_buf();
    for part in rel.split('/') {
        if craft::is_file_package(&full) {
            return None;
        }
        if part.is_empty() || part.starts_with('.') || part.ends_with(['.', ' ']) {
            return None;
        }
        if part
            .chars()
            .any(|c| "<>:\"|?*".contains(c) || c.is_control())
        {
            return None;
        }
        full.push(part);
        if linked_path(&full) {
            return None;
        }
    }
    Some(full)
}

fn linked_path(path: &std::path::Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.is_symlink()
}

fn query_param(query: &str, key: &str) -> String {
    for part in query.split('&') {
        if let Some((k, v)) = part.split_once('=') {
            if k == key {
                return decode(v).unwrap_or_default().to_string();
            }
        }
    }
    String::new()
}

fn visible_entry(entry: &walkdir::DirEntry) -> bool {
    entry.depth() == 0
        || (!entry.file_name().to_string_lossy().starts_with('.')
            && !linked_path(entry.path())
            && !entry.path().parent().is_some_and(craft::is_file_package))
}

fn list_files() -> Vec<FileEntry> {
    let root = vault_root();
    let mut out = Vec::new();
    for entry in WalkDir::new(&root)
        .into_iter()
        .filter_entry(visible_entry)
        .filter_map(|e| e.ok())
    {
        let p = entry.path();
        if p == root {
            continue;
        }
        let rel = p
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        out.push(FileEntry {
            path: rel,
            name,
            is_dir: entry.file_type().is_dir() && !craft::is_file_package(p),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn backlinks(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let paths = md_files();
    let target = resolve_link(name, &paths);
    for path in &paths {
        if Some(path) == target.as_ref() {
            continue;
        }
        let content = safe_path(path)
            .and_then(|p| fs::read_to_string(craft::document_path(&p)).ok())
            .unwrap_or_default();
        if target.is_some()
            && wiki_links(&content)
                .into_iter()
                .any(|link| resolve_wiki_link(link, &paths) == target)
        {
            out.push(path.clone());
        }
    }
    out
}

fn parse_outline(content: &str) -> Vec<OutlineItem> {
    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if hashes == 0 || hashes > 6 {
            continue;
        }
        let rest = &trimmed[hashes..];
        if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
            continue;
        }
        out.push(OutlineItem {
            level: hashes as u8,
            text: rest.trim().to_string(),
            line: i + 1,
        });
    }
    out
}

fn parse_properties(content: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        return map;
    }
    let mut i = 1;
    while i < lines.len() && lines[i].trim() != "---" {
        if let Some((k, v)) = lines[i].split_once(':') {
            let key = k.trim();
            if !key.is_empty() {
                map.insert(key.to_string(), v.trim().to_string());
            }
        }
        i += 1;
    }
    if i >= lines.len() {
        return BTreeMap::new();
    }
    map
}

fn md_files() -> Vec<String> {
    list_files()
        .into_iter()
        .filter(|f| !f.is_dir && f.path.to_lowercase().ends_with(".md"))
        .map(|f| f.path)
        .collect()
}

// Exclude inline and fenced code from links, backlinks, and automatic renames.
fn markdown_text_chunks(content: &str) -> Vec<&str> {
    let bytes = content.as_bytes();
    let mut chunks = Vec::new();
    let (mut cursor, mut plain_start) = (0, 0);
    while cursor < bytes.len() {
        if bytes[cursor] != b'`' && bytes[cursor] != b'~' {
            cursor += 1;
            continue;
        }
        let marker = bytes[cursor];
        let mut end = cursor + 1;
        while end < bytes.len() && bytes[end] == marker {
            end += 1;
        }
        if marker == b'~' && end - cursor < 3 {
            cursor = end;
            continue;
        }
        let delimiter = &content[cursor..end];
        if let Some(close) = content[end..].find(delimiter) {
            chunks.push(&content[plain_start..cursor]);
            cursor = end + close + delimiter.len();
            plain_start = cursor;
        } else if end - cursor >= 3 {
            chunks.push(&content[plain_start..cursor]);
            plain_start = content.len();
            break;
        } else {
            cursor = end;
        }
    }
    chunks.push(&content[plain_start..]);
    chunks
}

fn wiki_links(content: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    for chunk in markdown_text_chunks(content) {
        let mut remaining = chunk;
        while let Some(start) = remaining.find("[[") {
            let after = &remaining[start + 2..];
            if let Some(end) = after.find("]]") {
                out.push((remaining[..start].ends_with('@'), &after[..end]));
                remaining = &after[end + 2..];
            } else {
                break;
            }
        }
    }
    out
}

fn rewrite_links(content: &str, from: &str, to: &str, old_paths: &[String]) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    for chunk in markdown_text_chunks(content) {
        let start = chunk.as_ptr() as usize - content.as_ptr() as usize;
        output.push_str(&content[cursor..start]);
        output.push_str(&rewrite_plain_links(chunk, from, to, old_paths));
        cursor = start + chunk.len();
    }
    output.push_str(&content[cursor..]);
    output
}

fn rewrite_plain_links(content: &str, from: &str, to: &str, old_paths: &[String]) -> String {
    let mut output = String::new();
    let mut remaining = content;
    while let Some(start) = remaining.find("[[") {
        output.push_str(&remaining[..start + 2]);
        let after = &remaining[start + 2..];
        let Some(end) = after.find("]]") else {
            output.push_str(after);
            return output;
        };
        let inner = &after[..end];
        if remaining[..start].ends_with('@') {
            let renamed = decode(inner).ok().filter(|path| {
                path == from
                    || path
                        .strip_prefix(from)
                        .is_some_and(|tail| tail.starts_with('/'))
            });
            if let Some(path) = renamed {
                let new_path = format!("{}{}", to, &path[from.len()..]);
                output.push_str(&urlencoding::encode(&new_path));
            } else {
                output.push_str(inner);
            }
            output.push_str("]]");
            remaining = &after[end + 2..];
            continue;
        }
        let cut = inner.find(['#', '|']).unwrap_or(inner.len());
        let target = &inner[..cut];
        if let Some(resolved) = resolve_link(target.trim(), old_paths) {
            if resolved == from || resolved.starts_with(&format!("{}/", from)) {
                let new_path = format!("{}{}", to, &resolved[from.len()..]);
                let replacement = if target.to_lowercase().ends_with(".md") {
                    new_path
                } else {
                    new_path
                        .strip_suffix(".md")
                        .unwrap_or(&new_path)
                        .to_string()
                };
                output.push_str(&replacement);
                output.push_str(&inner[cut..]);
            } else {
                output.push_str(inner);
            }
        } else {
            output.push_str(inner);
        }
        output.push_str("]]");
        remaining = &after[end + 2..];
    }
    output.push_str(remaining);
    output
}

fn json_error(req: tiny_http::Request, code: u16, error: &str) {
    let _ = req.respond(
        Response::from_string(serde_json::json!({"error": error}).to_string())
            .with_status_code(code)
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
    );
}

fn resolve_link(link: &str, paths: &[String]) -> Option<String> {
    let mut cand = link.replace('\\', "/");
    cand = cand.trim_start_matches('/').to_string();
    if paths.contains(&cand) {
        return Some(cand);
    }
    if !cand.to_lowercase().ends_with(".md") {
        cand.push_str(".md");
    }
    if paths.iter().any(|p| p == &cand) {
        return Some(cand);
    }
    let base = link.rsplit('/').next().unwrap_or(link);
    let base_md = if base.to_lowercase().ends_with(".md") {
        base.to_string()
    } else {
        format!("{}.md", base)
    };
    for p in paths {
        let fname = p.rsplit('/').next().unwrap_or(p);
        if fname.eq_ignore_ascii_case(&base_md) {
            return Some(p.clone());
        }
    }
    None
}

fn resolve_wiki_link((encoded, raw): (bool, &str), paths: &[String]) -> Option<String> {
    if encoded {
        let path = decode(raw).ok()?;
        return paths
            .iter()
            .find(|candidate| candidate.as_str() == path)
            .cloned();
    }
    resolve_link(raw.split(['|', '#']).next()?.trim(), paths)
}

fn build_graph() -> Graph {
    let paths = md_files();
    let mut nodes = Vec::new();
    for p in &paths {
        let title = std::path::Path::new(p)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(p)
            .to_string();
        nodes.push(GraphNode {
            id: p.clone(),
            path: p.clone(),
            title,
        });
    }
    let mut edge_set: std::collections::BTreeSet<(String, String)> =
        std::collections::BTreeSet::new();
    for src in &paths {
        let full = match safe_path(src) {
            Some(f) => f,
            None => continue,
        };
        let content = fs::read_to_string(craft::document_path(&full)).unwrap_or_default();
        for inner in wiki_links(&content) {
            if let Some(target) = resolve_wiki_link(inner, &paths).filter(|path| path != src) {
                edge_set.insert((src.clone(), target));
            }
        }
    }
    let edges = edge_set
        .into_iter()
        .map(|(s, t)| GraphEdge {
            source: s,
            target: t,
        })
        .collect();
    Graph { nodes, edges }
}

fn workspace_file() -> Option<PathBuf> {
    let dir = vault_root().join(".obsidian");
    let file = dir.join("workspace.json");
    (!linked_path(&dir) && !linked_path(&file)).then_some(file)
}

fn handle(
    mut req: tiny_http::Request,
    config: &std::sync::Arc<server::Config>,
    writes: &std::sync::Arc<Mutex<()>>,
) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));

    if method == Method::Get && path == "/healthz" {
        return server::json(req, serde_json::json!({"ok":true}));
    }
    let account = if let Some(accounts) = &config.accounts {
        if !accounts.host(&req) {
            return json_error(req, 403, "Host no permitido");
        }
        if matches!(
            method,
            Method::Post | Method::Put | Method::Delete | Method::Patch
        ) && !accounts.origin(&req)
        {
            return json_error(req, 403, "Origen de solicitud no permitido");
        }
        match (&method, path) {
            (&Method::Get, "/login") => {
                let response = Response::from_string(LOGIN_HTML)
                    .with_header(Header::from_bytes("Content-Type","text/html; charset=utf-8").unwrap())
                    .with_header(Header::from_bytes("Cache-Control","no-store").unwrap())
                    .with_header(Header::from_bytes("Content-Security-Policy","default-src 'none'; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'").unwrap());
                let _ = req.respond(response);
                return;
            }
            (&Method::Post, "/api/session/login") => return accounts.login(req),
            (&Method::Post, "/api/session/logout") => return accounts.logout(req),
            _ => {}
        }
        let actor = accounts.actor(&req);
        if actor.is_none() && !(method == Method::Get && path.starts_with("/ui/")) {
            if method == Method::Get
                && matches!(path, "/" | "/index.html" | "/cerebro" | "/cerebro/")
            {
                let next = format!("/login?next={}", urlencoding::encode(&url));
                return accounts::redirect(req, &next);
            }
            return json_error(req, 401, "Inicia sesión");
        }
        actor
    } else if !config.authorized(&req) {
        return server::unauthorized(req);
    } else {
        None
    };
    if method == Method::Get && path == "/api/craft/changes" {
        if !server::trusted_origin(&req) {
            return json_error(req, 403, "Origen de solicitud no permitido");
        }
        return craft::watch(
            req,
            query_param(query, "path"),
            config.clone(),
            writes.clone(),
        );
    }
    if path == "/api/ai/settings" && matches!(method, Method::Get | Method::Put | Method::Delete) {
        if !server::trusted_origin(&req)
            || !config.allows_ai_settings(&req)
            || account.as_ref().is_some_and(|actor| !actor.owner)
        {
            return json_error(
                req,
                403,
                "Solo el propietario puede cambiar los ajustes de IA",
            );
        }
        return config.ai.edit(req);
    }
    if matches!(
        (&method, path),
        (&Method::Get, "/api/ai/status") | (&Method::Post, "/api/ai/chat")
    ) {
        if !server::trusted_origin(&req) {
            return json_error(req, 403, "Origen de solicitud no permitido");
        }
        if let (Some(accounts), Some(actor)) = (&config.accounts, &account) {
            if !actor.owner {
                let _guard = writes.lock().unwrap();
                if !accounts.allows(actor, &query_param(query, "path")) {
                    return json_error(req, 403, "Abre un documento compartido para usar la IA");
                }
            }
        }
        return if method == Method::Get {
            config.ai.status(req)
        } else {
            config.ai.send(req)
        };
    }
    let _guard = if matches!(method, Method::Post | Method::Put | Method::Delete)
        || matches!(
            path,
            "/api/craft/file"
                | "/api/craft/context"
                | "/api/download"
                | "/api/files"
                | "/api/craft/share"
                | "/api/file"
                | "/api/search"
                | "/api/backlinks"
                | "/api/outline"
                | "/api/properties"
                | "/api/graph"
        ) {
        if !server::trusted_origin(&req) {
            return json_error(req, 403, "Origen de solicitud no permitido");
        }
        Some(writes.lock().unwrap())
    } else {
        None
    };
    if let (Some(accounts), Some(actor)) = (&config.accounts, &account) {
        if !actor.owner {
            let office = matches!(
                (&method, path),
                (
                    &Method::Get,
                    "/api/craft/file" | "/api/craft/context" | "/api/download"
                ) | (&Method::Put, "/api/craft/file")
                    | (
                        &Method::Post,
                        "/api/craft/presence" | "/api/craft/participants"
                    )
            );
            let shell = method == Method::Get
                && (matches!(path, "/" | "/index.html" | "/api/hub" | "/api/files")
                    || path.starts_with("/ui/")
                    || ["wordcraft", "deckcraft", "gridcraft"]
                        .iter()
                        .any(|editor| path.starts_with(&format!("/craft/{editor}/"))));
            if !shell && !(office && accounts.allows(actor, &query_param(query, "path"))) {
                return json_error(req, 403, "Este archivo o acción no está compartido contigo");
            }
            if method == Method::Get && path == "/api/files" {
                return server::json(req, serde_json::json!(accounts.files(actor)));
            }
        }
        match (&method, path) {
            (&Method::Post, "/api/accounts") => return accounts.create_user(req, actor),
            (&Method::Get | &Method::Post, "/api/craft/share") => {
                return accounts.share(req, actor, query)
            }
            (&Method::Post, "/api/craft/participants")
                if accounts.allows(actor, &query_param(query, "path")) =>
            {
                return accounts.participants(req, actor, query)
            }
            _ => {}
        }
    }

    match (method, path) {
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            respond_html(req, "hub.html", HUB_HTML);
        }
        (Method::Get, "/cerebro") | (Method::Get, "/cerebro/") => {
            respond_html(req, "index.html", INDEX_HTML);
        }
        (Method::Get, "/ui/hub.js") => {
            respond_asset(req, "ui/hub.js", HUB_JS, "text/javascript; charset=utf-8");
        }
        (Method::Get, "/ui/references.js") => {
            respond_asset(
                req,
                "ui/references.js",
                REFERENCES_JS,
                "text/javascript; charset=utf-8",
            );
        }
        (Method::Get, "/ui/craft-editor.js") => {
            respond_asset(
                req,
                "ui/craft-editor.js",
                CRAFT_JS,
                "text/javascript; charset=utf-8",
            );
        }
        (Method::Get, "/ui/craft-merge.bundle.js") => {
            respond_asset(
                req,
                "ui/craft-merge.bundle.js",
                CRAFT_MERGE_JS,
                "text/javascript; charset=utf-8",
            );
        }
        (Method::Get, "/ui/accounts.js") => {
            respond_asset(
                req,
                "ui/accounts.js",
                ACCOUNT_JS,
                "text/javascript; charset=utf-8",
            );
        }
        (Method::Get, "/ui/hub.css") => {
            respond_asset(req, "ui/hub.css", HUB_CSS, "text/css; charset=utf-8");
        }
        (Method::Get, "/ui/chat.js") => {
            respond_asset(req, "ui/chat.js", CHAT_JS, "text/javascript; charset=utf-8");
        }
        (Method::Get, "/ui/chat.css") => {
            respond_asset(req, "ui/chat.css", CHAT_CSS, "text/css; charset=utf-8");
        }
        (Method::Get, "/ui/stageshow.css") => {
            respond_asset(
                req,
                "ui/stageshow.css",
                STAGESHOW_CSS,
                "text/css; charset=utf-8",
            );
        }
        (Method::Get, "/api/hub") => {
            let mut editors = craft::editors(&config.craft_root);
            let restricted = account.as_ref().is_some_and(|actor| !actor.owner);
            if restricted {
                editors.retain(|editor| editor["id"] != "pdfcraft");
            }
            server::json(
                req,
                serde_json::json!({"editors":editors,"tools":if restricted {&[][..]} else {&config.tools[..]},
                    "account":account.as_ref().map(|actor|serde_json::json!({"name":actor.name,"owner":actor.owner}))}),
            );
        }
        (Method::Get, path) if path.starts_with("/craft/") => {
            craft::asset(req, &config.craft_root, &path["/craft/".len()..]);
        }
        (Method::Get, "/api/craft/file") => craft::open(req, &query_param(query, "path")),
        (Method::Get, "/api/craft/context") => craft::context(req, &query_param(query, "path")),
        (Method::Put, "/api/craft/file") => craft::save(req, &query_param(query, "path")),
        (Method::Post, "/api/craft/presence") => craft::editor_presence(
            req,
            &query_param(query, "path"),
            &query_param(query, "session"),
            query_param(query, "leave") == "true",
        ),
        (Method::Get, "/api/download") => {
            let logical = query_param(query, "path");
            let Some(full) = safe_path(&logical)
                .map(|path| craft::document_path(&path))
                .filter(|path| path.is_file())
            else {
                return json_error(req, 404, "Archivo no encontrado");
            };
            let Ok(file) = fs::File::open(&full) else {
                return json_error(req, 404, "Archivo no encontrado");
            };
            let name = logical.rsplit('/').next().unwrap_or("documento.docx");
            let disposition = format!(
                "attachment; filename*=UTF-8''{}",
                urlencoding::encode(&name)
            );
            let _ = req.respond(
                Response::from_file(file)
                    .with_header(
                        Header::from_bytes("Content-Type", "application/octet-stream").unwrap(),
                    )
                    .with_header(Header::from_bytes("Content-Disposition", disposition).unwrap())
                    .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap())
                    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap()),
            );
        }
        (Method::Put, "/api/upload") | (Method::Post, "/api/documents") => {
            let rel = query_param(query, "path");
            let Some(full) = safe_path(&rel) else {
                return json_error(req, 400, "Ruta no válida");
            };
            let bytes = if path == "/api/documents" {
                let Some(template) = craft::template(&rel) else {
                    return json_error(req, 415, "Formato de documento no compatible");
                };
                template.to_vec()
            } else {
                match server::read_limited(req.as_reader(), server::MAX_FILE) {
                    Ok(bytes) => bytes,
                    Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {
                        return json_error(req, 413, "El archivo supera 32 MiB");
                    }
                    Err(_) => return json_error(req, 400, "No se pudo leer el archivo"),
                }
            };
            match craft::create_document(&full, &bytes) {
                Ok(()) => server::json(
                    req,
                    serde_json::json!({"ok":true,"revision":craft::revision(&bytes)}),
                ),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    json_error(req, 409, "Ya existe un archivo con ese nombre");
                }
                Err(error) => json_error(req, 500, &error.to_string()),
            }
        }
        (Method::Get, "/ui/app.css")
        | (Method::Get, "/ui/notion.css")
        | (Method::Get, "/ui/bundle.js") => {
            let (body, mime) = if path == "/ui/notion.css" {
                (NOTION_CSS, "text/css; charset=utf-8")
            } else if path.ends_with(".css") {
                (APP_CSS, "text/css; charset=utf-8")
            } else {
                (APP_JS, "text/javascript; charset=utf-8")
            };
            let relative = path.trim_start_matches('/');
            let _ = req.respond(
                Response::from_string(ui_asset(relative, body))
                    .with_header(Header::from_bytes("Content-Type", mime).unwrap())
                    .with_header(Header::from_bytes("Cache-Control", "no-cache").unwrap()),
            );
        }
        (Method::Get, "/api/files") => {
            let data = serde_json::to_string(&list_files()).unwrap();
            let resp = Response::from_string(data)
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
                .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
            let _ = req.respond(resp);
        }
        (Method::Get, "/api/file") => {
            let rel = query_param(query, "path");
            match safe_path(&rel).map(|path| craft::document_path(&path)) {
                Some(full) if full.is_file() => {
                    let Ok(file) = fs::File::open(&full) else {
                        return json_error(req, 404, "Archivo no encontrado");
                    };
                    let Ok(bytes) = server::read_limited(file, server::MAX_FILE) else {
                        return json_error(req, 413, "Archivo demasiado grande");
                    };
                    let Ok(content) = String::from_utf8(bytes) else {
                        return json_error(req, 415, "Abre este archivo desde el gestor");
                    };
                    #[derive(Serialize)]
                    struct R {
                        content: String,
                    }
                    let data = serde_json::to_string(&R { content }).unwrap();
                    let resp = Response::from_string(data).with_header(
                        Header::from_bytes("Content-Type", "application/json").unwrap(),
                    );
                    let _ = req.respond(resp);
                }
                _ => {
                    let body = "{\"error\":\"not found\"}".to_string();
                    let resp = Response::from_string(body)
                        .with_status_code(404)
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                    let _ = req.respond(resp);
                }
            }
        }
        (Method::Post, "/api/file") => {
            let wr = match server::read_json::<WriteReq>(&mut req, server::MAX_FILE) {
                Ok(value) => value,
                Err(status) => {
                    return json_error(req, status, "Solicitud no válida o demasiado grande")
                }
            };
            if config
                .accounts
                .as_ref()
                .is_some_and(|accounts| accounts.protects(&wr.path))
            {
                return json_error(req, 409, "Edita este archivo desde su editor ArtCraft");
            }
            let Some(full) = safe_path(&wr.path) else {
                return json_error(req, 400, "Ruta no válida");
            };
            if let Err(e) = craft::write_document(&full, wr.content.as_bytes()) {
                return json_error(req, 500, &e.to_string());
            }
            let _ = req.respond(
                Response::from_string("{\"ok\":true}")
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
            );
        }
        (Method::Delete, "/api/file") => {
            let rel = query_param(query, "path");
            if config
                .accounts
                .as_ref()
                .is_some_and(|accounts| accounts.protects(&rel))
            {
                return json_error(req, 409, "Retira primero el acceso compartido");
            }
            let Some(full) = safe_path(&rel) else {
                return json_error(req, 400, "Ruta no válida");
            };
            let removed = if craft::is_file_package(&full) {
                fs::remove_dir_all(&full)
            } else {
                fs::remove_file(&full)
            };
            if let Err(e) = removed {
                return json_error(req, 500, &e.to_string());
            }
            let _ = req.respond(
                Response::from_string("{\"ok\":true}")
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
            );
        }
        (Method::Post, "/api/folder") => {
            let fr = match server::read_json::<FolderReq>(&mut req, 64 * 1024) {
                Ok(value) => value,
                Err(status) => {
                    return json_error(req, status, "Solicitud no válida o demasiado grande")
                }
            };
            let Some(full) = safe_path(&fr.path) else {
                return json_error(req, 400, "Ruta no válida");
            };
            if full.exists() {
                return json_error(req, 409, "Ya existe un archivo o carpeta con ese nombre");
            }
            if let Err(e) = fs::create_dir_all(&full) {
                return json_error(req, 500, &e.to_string());
            }
            let _ = req.respond(
                Response::from_string("{\"ok\":true}")
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
            );
        }
        (Method::Post, "/api/rename") | (Method::Post, "/api/move") => {
            let rr = match server::read_json::<RenameReq>(&mut req, 64 * 1024) {
                Ok(value) => value,
                Err(status) => {
                    return json_error(req, status, "Solicitud no válida o demasiado grande")
                }
            };
            if config
                .accounts
                .as_ref()
                .is_some_and(|accounts| accounts.protects(&rr.from))
            {
                return json_error(req, 409, "Retira primero el acceso compartido");
            }
            let (Some(from), Some(to)) = (safe_path(&rr.from), safe_path(&rr.to)) else {
                return json_error(req, 400, "Ruta no válida");
            };
            if to.exists() {
                return json_error(req, 409, "Ya existe un archivo o carpeta con ese nombre");
            }
            if !from.exists() {
                return json_error(req, 404, "No se ha encontrado el archivo");
            }
            if craft::is_word_package(&from) && !rr.to.to_ascii_lowercase().ends_with(".docx") {
                return json_error(req, 400, "Conserva la extensión .docx del documento Word.");
            }
            let old_paths = md_files();
            // Read all affected links before changing the tree so aliases and headings survive.
            let updates: Vec<_> = old_paths
                .iter()
                .filter_map(|p| {
                    let full = safe_path(p)?;
                    let content = fs::read_to_string(craft::document_path(&full)).ok()?;
                    let updated = rewrite_links(&content, &rr.from, &rr.to, &old_paths);
                    if updated == content {
                        return None;
                    }
                    let target = if p == &rr.from || p.starts_with(&format!("{}/", rr.from)) {
                        format!("{}{}", rr.to, &p[rr.from.len()..])
                    } else {
                        p.clone()
                    };
                    Some((target, updated))
                })
                .collect();
            if let Some(parent) = to.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return json_error(req, 500, &e.to_string());
                }
            }
            if let Err(e) = craft::rename_document(&from, &to) {
                return json_error(req, 500, &e.to_string());
            }
            craft::move_word_jobs(&from, &to);
            for (target, content) in updates {
                if let Some(full) = safe_path(&target) {
                    if let Err(e) = craft::write_document(&full, content.as_bytes()) {
                        return json_error(
                            req,
                            500,
                            &format!("Archivo renombrado; no se pudo actualizar un enlace: {}", e),
                        );
                    }
                }
            }
            let _ = req.respond(
                Response::from_string("{\"ok\":true}")
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
            );
        }
        (Method::Delete, "/api/folder") => {
            let rel = query_param(query, "path");
            if config
                .accounts
                .as_ref()
                .is_some_and(|accounts| accounts.protects(&rel))
            {
                return json_error(req, 409, "Retira primero el acceso compartido");
            }
            let Some(full) = safe_path(&rel) else {
                return json_error(req, 400, "Ruta no válida");
            };
            if let Err(e) = fs::remove_dir_all(&full) {
                return json_error(req, 500, &e.to_string());
            }
            let _ = req.respond(
                Response::from_string("{\"ok\":true}")
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
            );
        }
        (Method::Get, "/api/search") => {
            let q = query_param(query, "q");
            let ql = q.to_lowercase();
            let mut results: Vec<SearchHit> = Vec::new();
            if !ql.is_empty() {
                for entry in WalkDir::new(vault_root())
                    .into_iter()
                    .filter_entry(visible_entry)
                    .filter_map(|e| e.ok())
                {
                    let p = entry.path();
                    let payload = craft::document_path(p);
                    if !payload.is_file() {
                        continue;
                    }
                    let rel = p
                        .strip_prefix(&vault_root())
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    let extension = p
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or_default();
                    if !matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "md" | "markdown" | "txt" | "canvas" | "base"
                    ) {
                        continue;
                    }
                    let content = fs::File::open(payload)
                        .and_then(|file| server::read_limited(file, server::MAX_FILE))
                        .ok()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                        .unwrap_or_default();
                    let path_match = rel.to_lowercase().contains(&ql);
                    let mut snippet = String::new();
                    let mut content_match = false;
                    for line in content.lines() {
                        if line.to_lowercase().contains(&ql) {
                            content_match = true;
                            let t = line.trim();
                            snippet = if t.chars().count() > 120 {
                                t.chars().take(120).collect()
                            } else {
                                t.to_string()
                            };
                            break;
                        }
                    }
                    if path_match || content_match {
                        results.push(SearchHit { path: rel, snippet });
                    }
                }
            }
            let data = serde_json::to_string(&results).unwrap();
            let resp = Response::from_string(data)
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(resp);
        }
        (Method::Get, "/api/backlinks") => {
            let name = query_param(query, "name");
            let data = serde_json::to_string(&backlinks(&name)).unwrap();
            let resp = Response::from_string(data)
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(resp);
        }
        (Method::Get, "/api/outline") => {
            let rel = query_param(query, "path");
            let out = match safe_path(&rel).map(|path| craft::document_path(&path)) {
                Some(full) if full.is_file() => {
                    let content = fs::read_to_string(&full).unwrap_or_default();
                    parse_outline(&content)
                }
                _ => Vec::new(),
            };
            let data = serde_json::to_string(&out).unwrap();
            let resp = Response::from_string(data)
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(resp);
        }
        (Method::Get, "/api/properties") => {
            let rel = query_param(query, "path");
            let props = match safe_path(&rel).map(|path| craft::document_path(&path)) {
                Some(full) if full.is_file() => {
                    let content = fs::read_to_string(&full).unwrap_or_default();
                    parse_properties(&content)
                }
                _ => BTreeMap::new(),
            };
            let data = serde_json::to_string(&props).unwrap();
            let resp = Response::from_string(data)
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(resp);
        }
        (Method::Get, "/api/graph") => {
            let data = serde_json::to_string(&build_graph()).unwrap();
            let resp = Response::from_string(data)
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(resp);
        }
        (Method::Get, "/api/workspace") => {
            let Some(file) = workspace_file() else {
                return json_error(req, 403, "Ruta de espacio no válida");
            };
            let mut out = String::from("{}");
            if let Ok(content) = fs::read_to_string(&file) {
                if !content.trim().is_empty() {
                    if serde_json::from_str::<serde_json::Value>(&content).is_ok() {
                        out = content;
                    }
                }
            }
            let resp = Response::from_string(out)
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(resp);
        }
        (Method::Post, "/api/workspace") => {
            let body = match server::read_json::<serde_json::Value>(&mut req, 1024 * 1024) {
                Ok(value) if value.is_object() => value.to_string(),
                Ok(_) => return json_error(req, 400, "Se esperaba un objeto JSON"),
                Err(status) => return json_error(req, status, "JSON no válido o demasiado grande"),
            };
            let Some(file) = workspace_file() else {
                return json_error(req, 403, "Ruta de espacio no válida");
            };
            if let Err(e) = server::write_atomic(&file, body.as_bytes(), true) {
                return json_error(req, 500, &e.to_string());
            }
            let _ = req.respond(
                Response::from_string("{\"ok\":true}")
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
            );
        }
        _ => {
            let body = "{\"error\":\"not found\"}".to_string();
            let resp = Response::from_string(body)
                .with_status_code(404)
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(resp);
        }
    }
}

fn respond_asset(req: tiny_http::Request, path: &str, bundled: &str, mime: &str) {
    let _ = req.respond(
        Response::from_string(ui_asset(path, bundled))
            .with_header(Header::from_bytes("Content-Type", mime).unwrap())
            .with_header(Header::from_bytes("Cache-Control", "no-cache").unwrap())
            .with_header(Header::from_bytes("Referrer-Policy", "same-origin").unwrap()),
    );
}

fn respond_html(req: tiny_http::Request, path: &str, bundled: &str) {
    respond_asset(req, path, bundled, "text/html; charset=utf-8");
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "user-add") {
        let name = args.get(1).ok_or("Usa user-add NOMBRE")?;
        let password = if args.get(2).is_some_and(|arg| arg == "--password-stdin") {
            let mut value = String::new();
            std::io::stdin().read_line(&mut value)?;
            value.trim_end_matches(['\n', '\r']).to_owned()
        } else {
            rpassword::prompt_password("Contraseña: ")?
        };
        accounts::add_user(name, &password)?;
        println!("Cuenta creada: {name}");
        return Ok(());
    }
    let config = std::sync::Arc::new(server::Config::from_env()?);
    fs::create_dir_all(vault_root())?;
    let server = Server::http(config.address)?;
    let writes = std::sync::Arc::new(Mutex::new(()));
    craft::prepare_existing_files();
    let index_writes = std::sync::Arc::clone(&writes);
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        craft::index_ready_words(&index_writes);
    });
    println!("Super Cerebro running at http://{}", config.address);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for req in server.incoming_requests() {
                    handle(req, &config, &writes);
                }
            });
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_folder_preserves_encoded_file_mentions() {
        let original = "@[[Equipo%2FInforme%20%231.docx]] / @[[Equipos%2FInforme.pdf]] / `@[[Equipo%2FInforme.pdf]]`";
        let updated = rewrite_links(original, "Equipo", "Archivo", &[]);
        assert_eq!(updated, "@[[Archivo%2FInforme%20%231.docx]] / @[[Equipos%2FInforme.pdf]] / `@[[Equipo%2FInforme.pdf]]`");
    }
    #[test]
    fn rename_preserves_alias_and_heading() {
        let paths = vec!["Ideas/Nota.md".to_string(), "Inicio.md".to_string()];
        assert_eq!(
            rewrite_links(
                "[[Nota#Sección|mi idea]] y [[Inicio]]",
                "Ideas/Nota.md",
                "Proyectos/Nueva.md",
                &paths
            ),
            "[[Proyectos/Nueva#Sección|mi idea]] y [[Inicio]]"
        );
    }

    #[test]
    fn moving_folder_updates_full_and_short_links() {
        let paths = vec!["Ideas/A.md".to_string(), "Ideas/B.md".to_string()];
        assert_eq!(
            rewrite_links(
                "[[Ideas/A.md]] [[B]] [[Sin crear]]",
                "Ideas",
                "Archivo",
                &paths
            ),
            "[[Archivo/A.md]] [[Archivo/B]] [[Sin crear]]"
        );
    }

    #[test]
    fn invalid_paths_cannot_target_root_or_parent() {
        for path in [
            "",
            "/",
            "../a.md",
            "a/../b.md",
            ".obsidian/workspace.json",
            "C:/archivo.md",
            "a//b.md",
        ] {
            assert!(safe_path(path).is_none(), "{}", path);
        }
        assert!(safe_path("Ideas/Una nota.md").is_some());
        assert!(safe_path("Notas/Versión 1..2.md").is_some());
    }

    #[test]
    fn unfinished_link_does_not_drop_text() {
        assert_eq!(
            rewrite_links("antes [[sin cerrar", "A.md", "B.md", &[]),
            "antes [[sin cerrar"
        );
    }

    #[test]
    fn graph_resolves_encoded_links_to_exact_notes() {
        let paths = vec!["Ideas/A #2 [revisión].md".to_string()];
        let links = wiki_links("@[[Ideas%2FA%20%232%20%5Brevisi%C3%B3n%5D.md]] @[[Informe.docx]]");
        assert_eq!(resolve_wiki_link(links[0], &paths), Some(paths[0].clone()));
        assert_eq!(resolve_wiki_link(links[1], &paths), None);
    }

    #[test]
    fn code_examples_are_not_links_and_are_never_renamed() {
        let text = "[[A]] `[[A]]`\n```md\n[[A]]\n```\n[[A#Título|alias]]";
        assert_eq!(
            wiki_links(text),
            vec![(false, "A"), (false, "A#Título|alias")]
        );
        assert_eq!(
            rewrite_links(text, "A.md", "B.md", &["A.md".to_string()]),
            "[[B]] `[[A]]`\n```md\n[[A]]\n```\n[[B#Título|alias]]"
        );
    }
}
