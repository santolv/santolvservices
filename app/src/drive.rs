use crate::{
    accounts,
    cerebro::{md_files, rewrite_links},
    craft, json_error, query_param, respond_asset, respond_html, server,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tiny_http::{Header, Method, Request, Response};
use walkdir::WalkDir;
mod preview;

const HUB_HTML: &str = include_str!("hub.html");
const HUB_JS: &str = include_str!("ui/hub.js");
const HUB_CSS: &str = include_str!("ui/hub.css");

#[derive(Serialize, Deserialize)]
pub(crate) struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
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

pub(crate) fn vault_root() -> PathBuf {
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

pub(crate) fn safe_path(rel: &str) -> Option<PathBuf> {
    safe_path_at(&vault_root(), rel)
}

pub(crate) fn safe_path_at(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.replace('\\', "/");
    if rel.is_empty() {
        return None;
    }
    let mut full = root.to_path_buf();
    for part in rel.split('/') {
        if is_file_package(&full) {
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

pub(crate) fn linked_path(path: &std::path::Path) -> bool {
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

pub(crate) fn visible_entry(entry: &walkdir::DirEntry) -> bool {
    entry.depth() == 0
        || (!entry.file_name().to_string_lossy().starts_with('.')
            && !linked_path(entry.path())
            && !entry.path().parent().is_some_and(is_file_package))
}

pub(crate) fn list_files() -> Vec<FileEntry> {
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
            is_dir: entry.file_type().is_dir() && !is_file_package(p),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

const FILE_MARKER: &str = ".archivo";
const WORD_BODY: &str = ".wordcraft.docx";

pub fn is_word_package(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
        && path.is_dir()
        && path.join(WORD_BODY).is_file()
        && !crate::linked_path(&path.join(WORD_BODY))
}

fn packaged_file(path: &Path) -> Option<PathBuf> {
    if !path.is_dir() || crate::linked_path(path) {
        return None;
    }
    let marker = path.join(FILE_MARKER);
    if crate::linked_path(&marker) || !marker.is_file() {
        return None;
    }
    let name =
        String::from_utf8(server::read_limited(File::open(marker).ok()?, 1024).ok()?).ok()?;
    if name.is_empty()
        || name.starts_with('.')
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|ch| ch.is_control() || "/\\<>:\"|?*".contains(ch))
    {
        return None;
    }
    [path.join(name), path.join(path.file_name()?)]
        .into_iter()
        .find(|payload| payload.is_file() && !crate::linked_path(payload))
}

pub fn is_file_package(path: &Path) -> bool {
    is_word_package(path) || packaged_file(path).is_some()
}

pub fn document_path(path: &Path) -> PathBuf {
    if is_word_package(path) {
        path.join(WORD_BODY)
    } else {
        packaged_file(path).unwrap_or_else(|| path.to_path_buf())
    }
}

pub(crate) fn prepare_existing_files() {
    // Recover an interrupted migration before exposing the vault to readers.
    let root = crate::vault_root();
    let staging = |name: &str| name.starts_with(".wordcraft-") || name.starts_with(".sc-package-");
    for entry in walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            entry.depth() == 0
                || (entry.file_type().is_dir()
                    && !crate::linked_path(entry.path())
                    && (staging(&name) || !is_file_package(entry.path()))
                    && (!name.starts_with('.') || staging(&name)))
        })
        .filter_map(Result::ok)
        .filter(|entry| staging(&entry.file_name().to_string_lossy()))
    {
        let previous = entry.path().join(".previous.docx");
        let marker = entry.path().join(".restore-name");
        if !previous.is_file() || crate::linked_path(&previous) || crate::linked_path(&marker) {
            continue;
        }
        let Ok(name) = fs::read_to_string(marker) else {
            continue;
        };
        if name.contains(['/', '\\']) {
            continue;
        }
        if let Some(target) = entry
            .path()
            .parent()
            .and_then(|parent| safe_path_at(parent, &name))
            .filter(|target| !target.exists())
        {
            if let Err(error) = fs::rename(&previous, &target) {
                eprintln!("No se pudo recuperar {}: {error}", previous.display());
            }
        }
    }
    for entry in list_files().into_iter().filter(|entry| !entry.is_dir) {
        let Some(path) = safe_path(&entry.path) else {
            continue;
        };
        if path.is_file() {
            if let Err(error) = install_package(&path, None, true) {
                eprintln!("No se pudo empaquetar {}: {error}", entry.path);
            }
        }
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
            && !craft::word_index_current(&path)
        {
            craft::queue_text(&path);
        }
    }
}

pub fn create_document(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    install_package(path, Some(bytes), false)?;
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
    {
        craft::queue_text(path);
    }
    Ok(())
}

pub fn write_document(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if is_file_package(path) {
        server::write_atomic(&document_path(path), bytes, true)
    } else if path.is_file() {
        install_package(path, Some(bytes), true)
    } else {
        create_document(path, bytes)
    }
}

pub fn rename_document(from: &Path, to: &Path) -> std::io::Result<()> {
    let payload = packaged_file(from).and_then(|path| path.file_name().map(|name| name.to_owned()));
    fs::rename(from, to)?;
    if let Some(old_name) = payload {
        let new_name = to
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ErrorKind::InvalidInput)?;
        if old_name != new_name {
            let old = to.join(&old_name);
            let new = to.join(new_name);
            if let Err(error) = fs::rename(&old, &new) {
                fs::rename(to, from)?;
                return Err(error);
            }
            if let Err(error) =
                server::write_atomic(&to.join(FILE_MARKER), new_name.as_bytes(), true)
            {
                fs::rename(&new, &old)?;
                fs::rename(to, from)?;
                return Err(error);
            }
        }
    }
    Ok(())
}

pub(crate) fn install_package(
    path: &Path,
    bytes: Option<&[u8]>,
    migrate: bool,
) -> std::io::Result<()> {
    if !migrate && path.exists() {
        return Err(ErrorKind::AlreadyExists.into());
    }
    let parent = path.parent().ok_or(ErrorKind::InvalidInput)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ErrorKind::InvalidInput)?;
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".sc-package-")
        .tempdir_in(parent)?;
    let word = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"));
    let payload = staging.path().join(if word { WORD_BODY } else { name });
    if let Some(bytes) = bytes {
        server::write_atomic(&payload, bytes, false)?;
    } else {
        fs::copy(path, &payload)?;
        fs::OpenOptions::new()
            .write(true)
            .open(&payload)?
            .sync_all()?;
    }
    if !word {
        server::write_atomic(&staging.path().join(FILE_MARKER), name.as_bytes(), false)?;
    }
    let previous = staging.path().join(".previous.docx");
    if migrate {
        server::write_atomic(
            &staging.path().join(".restore-name"),
            path.file_name()
                .ok_or(ErrorKind::InvalidInput)?
                .to_string_lossy()
                .as_bytes(),
            false,
        )?;
        fs::rename(path, &previous)?;
    }
    if let Err(error) = fs::rename(staging.path(), path) {
        if migrate && fs::rename(&previous, path).is_err() {
            eprintln!(
                "WordCraft: documento conservado en {}",
                staging.keep().display()
            );
        }
        return Err(error);
    }
    if migrate {
        let _ = fs::remove_file(path.join(".previous.docx"));
        let _ = fs::remove_file(path.join(".restore-name"));
    }
    Ok(())
}

pub(crate) fn handle(
    mut req: Request,
    method: Method,
    path: &str,
    query: &str,
    account: Option<&accounts::Actor>,
    config: &server::Config,
) {
    match (method, path) {
        (Method::Get, "/api/preview") => preview::respond(req, &query_param(query, "path")),
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            respond_html(req, "hub.html", HUB_HTML);
        }
        (Method::Get, "/ui/hub.js") => {
            respond_asset(req, "ui/hub.js", HUB_JS, "text/javascript; charset=utf-8");
        }
        (Method::Get, "/ui/hub.css") => {
            respond_asset(req, "ui/hub.css", HUB_CSS, "text/css; charset=utf-8");
        }
        (Method::Get, "/api/hub") => {
            let mut editors = craft::editors(&config.craft_root);
            let restricted = account.is_some_and(|actor| !actor.owner);
            if restricted {
                editors.retain(|editor| editor["id"] != "pdfcraft");
            }
            server::json(
                req,
                serde_json::json!({"editors":editors,"tools":if restricted {&[][..]} else {&config.tools[..]},
                    "account":account.map(|actor|serde_json::json!({"name":actor.name,"owner":actor.owner}))}),
            );
        }
        (Method::Get, "/api/download") => {
            let logical = query_param(query, "path");
            let Some(full) = safe_path(&logical)
                .map(|path| document_path(&path))
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
            match create_document(&full, &bytes) {
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
        (Method::Get, "/api/files") => {
            let data = serde_json::to_string(&list_files()).unwrap();
            let resp = Response::from_string(data)
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
                .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
            let _ = req.respond(resp);
        }
        (Method::Get, "/api/file") => {
            let rel = query_param(query, "path");
            match safe_path(&rel).map(|path| document_path(&path)) {
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
            if let Err(e) = write_document(&full, wr.content.as_bytes()) {
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
            let removed = if is_file_package(&full) {
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
            if is_word_package(&from) && !rr.to.to_ascii_lowercase().ends_with(".docx") {
                return json_error(req, 400, "Conserva la extensión .docx del documento Word.");
            }
            let old_paths = md_files();
            // Read all affected links before changing the tree so aliases and headings survive.
            let updates: Vec<_> = old_paths
                .iter()
                .filter_map(|p| {
                    let full = safe_path(p)?;
                    let content = fs::read_to_string(document_path(&full)).ok()?;
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
            if let Err(e) = rename_document(&from, &to) {
                return json_error(req, 500, &e.to_string());
            }
            craft::move_word_jobs(&from, &to);
            for (target, content) in updates {
                if let Some(full) = safe_path(&target) {
                    if let Err(e) = write_document(&full, content.as_bytes()) {
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
        _ => json_error(req, 404, "Ruta no encontrada"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_packages_keep_names_and_payloads_through_migration_and_rename() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("Nota.md");
        std::fs::write(&original, "texto antiguo").unwrap();
        super::install_package(&original, None, true).unwrap();
        assert!(super::is_file_package(&original));
        assert!(safe_path_at(root.path(), "Nota.md/Nota.md").is_none());
        assert_eq!(
            std::fs::read_to_string(original.join("Nota.md")).unwrap(),
            "texto antiguo"
        );

        super::write_document(&original, b"editado").unwrap();
        let renamed = root.path().join("Nota nueva.md");
        super::rename_document(&original, &renamed).unwrap();
        assert!(!original.exists());
        assert_eq!(
            super::document_path(&renamed),
            renamed.join("Nota nueva.md")
        );
        assert_eq!(
            std::fs::read(super::document_path(&renamed)).unwrap(),
            b"editado"
        );
        assert!(super::create_document(&renamed, b"duplicado").is_err());
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
}
