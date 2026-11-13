use crate::{
    drive::{self, list_files, safe_path, vault_root, visible_entry},
    json_error, linked_path, query_param, respond_asset, respond_html, server,
};
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::PathBuf};
use tiny_http::{Header, Method, Request, Response};
use urlencoding::decode;
use walkdir::WalkDir;

const INDEX_HTML: &str = include_str!("index.html");
const NOTION_CSS: &str = include_str!("ui/notion.css");
const APP_JS: &str = include_str!("ui/bundle.js");

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

fn backlinks(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let paths = md_files();
    let target = resolve_link(name, &paths);
    for path in &paths {
        if Some(path) == target.as_ref() {
            continue;
        }
        let content = safe_path(path)
            .and_then(|p| fs::read_to_string(drive::document_path(&p)).ok())
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

pub(crate) fn md_files() -> Vec<String> {
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

pub(crate) fn rewrite_links(content: &str, from: &str, to: &str, old_paths: &[String]) -> String {
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
        let content = fs::read_to_string(drive::document_path(&full)).unwrap_or_default();
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

pub(crate) fn handle(mut req: Request, method: Method, path: &str, query: &str) {
    match (method, path) {
        (Method::Get, "/cerebro") | (Method::Get, "/cerebro/") => {
            respond_html(req, "index.html", INDEX_HTML);
        }
        (Method::Get, "/ui/notion.css") => {
            respond_asset(req, "ui/notion.css", NOTION_CSS, "text/css; charset=utf-8")
        }
        (Method::Get, "/ui/bundle.js") => respond_asset(
            req,
            "ui/bundle.js",
            APP_JS,
            "text/javascript; charset=utf-8",
        ),
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
                    let payload = drive::document_path(p);
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
            let out = match safe_path(&rel).map(|path| drive::document_path(&path)) {
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
            let props = match safe_path(&rel).map(|path| drive::document_path(&path)) {
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
        _ => json_error(req, 404, "Ruta no encontrada"),
    }
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
