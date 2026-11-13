#[cfg(test)]
use crate::drive::{create_document, is_word_package};
use crate::{
    drive::{
        document_path, install_package, is_file_package, safe_path, safe_path_at, write_document,
    },
    json_error, query_param, respond_asset, server,
};
use quick_xml::{events::Event, name::ResolveResult, NsReader};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Cursor, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    mpsc, Arc,
};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response};

const EDITORS: [(&str, &str, &str); 4] = [
    ("wordcraft", "WordCraft", "docx"),
    ("deckcraft", "DeckCraft", "pptx"),
    ("gridcraft", "GridCraft", "xlsx"),
    ("pdfcraft", "PdfCraft", "pdf"),
];

pub fn editors(root: &Path) -> Vec<Value> {
    EDITORS
        .iter()
        .map(|(id, name, extension)| {
            let available =
                safe_path_at(root, &format!("{id}/index.html")).is_some_and(|file| file.is_file());
            json!({ "id": id, "name": name, "extension": extension, "available": available })
        })
        .collect()
}

pub fn asset(req: Request, root: &Path, path: &str) {
    let Some((id, relative)) = path.split_once('/') else {
        return json_error(req, 404, "Editor no disponible");
    };
    if !EDITORS.iter().any(|editor| editor.0 == id) {
        return json_error(req, 404, "Editor no disponible");
    }
    let mime = match extension(relative).as_str() {
        "html" if relative == "index.html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "ico" => "image/x-icon",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        _ => return json_error(req, 404, "Recurso no disponible"),
    };
    let Some(full) = safe_path_at(root, path).filter(|file| file.is_file()) else {
        return json_error(req, 404, "Recurso no disponible");
    };
    let Ok(file) = File::open(full) else {
        return json_error(req, 404, "Recurso no disponible");
    };
    let _ = req.respond(
        Response::from_file(file)
            .with_header(Header::from_bytes("Content-Type", mime).unwrap())
            .with_header(Header::from_bytes("Cache-Control", "private, no-cache").unwrap())
            .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap())
            .with_header(Header::from_bytes("X-Frame-Options", "SAMEORIGIN").unwrap())
            .with_header(Header::from_bytes("Referrer-Policy", "same-origin").unwrap()),
    );
}

pub fn template(path: &str) -> Option<&'static [u8]> {
    match extension(path).as_str() {
        "docx" => Some(include_bytes!("blank.docx")),
        "pptx" => Some(include_bytes!("blank.pptx")),
        "xlsx" => Some(include_bytes!("blank.xlsx")),
        "pdf" => Some(include_bytes!("blank.pdf")),
        _ => None,
    }
}

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

const WORD_TEXT: &str = "texto.md";
const WORD_REVISION: &str = ".texto-revision";
const EDITOR_LEASE: Duration = Duration::from_secs(120);

#[derive(Default)]
struct WordJob {
    editors: HashMap<String, Instant>,
    due: Option<Instant>,
}
fn jobs() -> &'static Mutex<HashMap<PathBuf, WordJob>> {
    static JOBS: OnceLock<Mutex<HashMap<PathBuf, WordJob>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}

pub(crate) fn word_index_current(path: &Path) -> bool {
    if !is_file_package(path) {
        return false;
    }
    let marker = path.join(WORD_REVISION);
    let text = path.join(WORD_TEXT);
    if crate::linked_path(&marker) || crate::linked_path(&text) || !text.is_file() {
        return false;
    }
    let modified = |file: &Path| fs::metadata(file).and_then(|meta| meta.modified());
    matches!((modified(&marker),modified(&document_path(path))),(Ok(index),Ok(body)) if index>=body)
}

pub(crate) fn queue_text(path: &Path) {
    jobs().lock().unwrap().entry(path.into()).or_default().due =
        Some(Instant::now() + Duration::from_secs(2));
}

pub fn editor_presence(req: Request, path: &str, session: &str, leaving: bool) {
    let Some(path) = safe_path(path) else {
        return json_error(req, 400, "Ruta no válida");
    };
    if extension(&path.to_string_lossy()) != "docx"
        || !document_path(&path).is_file()
        || session.len() != 32
        || !session.bytes().all(|ch| ch.is_ascii_hexdigit())
    {
        return json_error(req, 400, "Sesión de WordCraft no válida");
    }
    let mut jobs = jobs().lock().unwrap();
    let job = jobs.entry(path).or_default();
    job.editors.retain(|_, expiry| *expiry > Instant::now());
    if leaving {
        job.editors.remove(session);
    } else if job.editors.len() < 128 || job.editors.contains_key(session) {
        job.editors
            .insert(session.into(), Instant::now() + EDITOR_LEASE);
    }
    drop(jobs);
    server::json(req, json!({"ok":true}));
}

pub fn move_word_jobs(from: &Path, to: &Path) {
    let mut jobs = jobs().lock().unwrap();
    let paths: Vec<_> = jobs
        .keys()
        .filter(|path| path.starts_with(from))
        .cloned()
        .collect();
    for path in paths {
        let relative = path.strip_prefix(from).unwrap();
        let target = if relative.as_os_str().is_empty() {
            to.into()
        } else {
            to.join(relative)
        };
        if let Some(job) = jobs.remove(&path) {
            jobs.insert(target, job);
        }
    }
}

pub fn index_ready_words(writes: &Mutex<()>) {
    let now = Instant::now();
    let mut ready = Vec::new();
    jobs().lock().unwrap().retain(|path, job| {
        job.editors.retain(|_, expiry| *expiry > now);
        if job.editors.is_empty() {
            if let Some(due) = job.due.filter(|due| *due <= now) {
                ready.push((path.clone(), due));
            }
        }
        !job.editors.is_empty() || job.due.is_some()
    });
    for (path, due) in ready {
        let still_ready = |job: &WordJob| {
            job.due == Some(due) && !job.editors.values().any(|expiry| *expiry > Instant::now())
        };
        let snapshot = {
            let _guard = writes.lock().unwrap();
            if !jobs().lock().unwrap().get(&path).is_some_and(still_ready) {
                continue;
            }
            read_document(&document_path(&path))
        };
        let extracted = snapshot.and_then(|bytes| {
            read_docx_text_with_limit(&bytes, None).map(|(text, _)| (bytes, text))
        });
        let _guard = writes.lock().unwrap();
        {
            let mut jobs = jobs().lock().unwrap();
            let Some(job) = jobs.get_mut(&path).filter(|job| still_ready(job)) else {
                continue;
            };
            job.due = None;
        }
        let Ok((bytes, text)) = extracted else {
            continue;
        };
        if !is_file_package(&path) && install_package(&path, Some(&bytes), true).is_err() {
            continue;
        }
        let result =
            server::write_atomic(&path.join(WORD_TEXT), text.as_bytes(), true).and_then(|()| {
                server::write_atomic(&path.join(WORD_REVISION), revision(&bytes).as_bytes(), true)
            });
        if let Err(error) = result {
            eprintln!("WordCraft: no se pudo actualizar el índice: {error}");
        }
    }
}

fn valid_format(path: &str, bytes: &[u8]) -> bool {
    match extension(path).as_str() {
        "docx" | "pptx" | "xlsx" => bytes.starts_with(b"PK\x03\x04"),
        "pdf" => bytes.starts_with(b"%PDF-"),
        _ => false,
    }
}

pub fn revision(bytes: &[u8]) -> String {
    format!("\"{:x}\"", Sha256::digest(bytes))
}

type Subscribers = HashMap<PathBuf, Vec<(u64, mpsc::SyncSender<()>)>>;
static SUBSCRIBERS: OnceLock<Mutex<Subscribers>> = OnceLock::new();
static WATCH_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_WATCH: AtomicU64 = AtomicU64::new(0);

fn subscribers() -> &'static Mutex<Subscribers> {
    SUBSCRIBERS.get_or_init(Mutex::default)
}

pub(crate) fn notify_document(path: &Path) {
    let Ok(key) = fs::canonicalize(path) else {
        return;
    };
    if let Some(waiters) = subscribers().lock().unwrap().remove(&key) {
        for (_, sender) in waiters {
            let _ = sender.try_send(());
        }
    }
}

struct Subscription {
    key: PathBuf,
    id: u64,
    receiver: mpsc::Receiver<()>,
}

impl Subscription {
    fn new(key: PathBuf) -> Self {
        let id = NEXT_WATCH.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::sync_channel(1);
        subscribers()
            .lock()
            .unwrap()
            .entry(key.clone())
            .or_default()
            .push((id, sender));
        Self { key, id, receiver }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut entries = subscribers().lock().unwrap();
        if let Some(waiters) = entries.get_mut(&self.key) {
            waiters.retain(|(id, _)| *id != self.id);
            if waiters.is_empty() {
                entries.remove(&self.key);
            }
        }
    }
}

struct WatchSlot;

impl Drop for WatchSlot {
    fn drop(&mut self) {
        WATCH_COUNT.fetch_sub(1, Ordering::Relaxed);
    }
}

pub fn watch(req: Request, path: String, config: Arc<server::Config>, writes: Arc<Mutex<()>>) {
    if !matches!(extension(&path).as_str(), "docx" | "pptx" | "xlsx") {
        return json_error(req, 415, "Formato sin colaboración");
    }
    let previous = WATCH_COUNT.fetch_add(1, Ordering::Relaxed);
    let slot = WatchSlot;
    if previous >= 64 {
        return json_error(req, 429, "Demasiadas sesiones esperando cambios");
    }
    let _ = std::thread::Builder::new().spawn(move || {
        let _slot = slot;
        let read = || {
            if let Some(accounts) = &config.accounts {
                let actor = accounts.actor(&req).ok_or((401, "Inicia sesión"))?;
                if !accounts.allows(&actor, &path) {
                    return Err((403, "Este archivo no está compartido contigo"));
                }
            }
            let full = safe_path(&path).ok_or((400, "Ruta no válida"))?;
            let bytes = read_document(&document_path(&full))?;
            if !valid_format(&path, &bytes) {
                return Err((415, "Formato de documento no válido"));
            }
            Ok((full, bytes))
        };
        let initial = {
            let _guard = writes.lock().unwrap();
            read().map(|(full, bytes)| {
                let tag = revision(&bytes);
                if server::header(&req, "If-None-Match") != Some(tag.as_str()) {
                    return Ok((bytes, tag));
                }
                Err(fs::canonicalize(full).map(Subscription::new))
            })
        };
        let subscription = match initial {
            Err((code, message)) => return json_error(req, code, message),
            Ok(Ok((bytes, tag))) => return respond_file(req, bytes, tag),
            Ok(Err(Ok(subscription))) => subscription,
            Ok(Err(Err(_))) => return json_error(req, 404, "Archivo no encontrado"),
        };
        let _ = subscription.receiver.recv_timeout(Duration::from_secs(20));
        let result = {
            let _guard = writes.lock().unwrap();
            read()
        };
        match result {
            Ok((_, bytes)) => {
                let tag = revision(&bytes);
                respond_file(req, bytes, tag);
            }
            Err((code, message)) => json_error(req, code, message),
        }
    });
}

pub fn open(req: Request, path: &str) {
    let Some(full) = safe_path(path) else {
        return json_error(req, 400, "Ruta no válida");
    };
    let bytes = match read_document(&document_path(&full)) {
        Ok(bytes) => bytes,
        Err((status, message)) => return json_error(req, status, message),
    };
    if !valid_format(path, &bytes) {
        return json_error(req, 415, "Formato de documento no válido");
    }
    let tag = revision(&bytes);
    if server::header(&req, "If-None-Match") != Some(tag.as_str())
        && extension(path) == "docx"
        && !word_index_current(&full)
    {
        queue_text(&full);
    }
    respond_file(req, bytes, tag);
}

fn respond_file(req: Request, bytes: Vec<u8>, tag: String) {
    if server::header(&req, "If-None-Match") == Some(tag.as_str()) {
        let _ = req.respond(
            Response::empty(304)
                .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                .with_header(Header::from_bytes("ETag", tag).unwrap()),
        );
        return;
    }
    let _ = req.respond(
        Response::from_data(bytes)
            .with_header(Header::from_bytes("Content-Type", "application/octet-stream").unwrap())
            .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap())
            .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
            .with_header(Header::from_bytes("ETag", tag).unwrap()),
    );
}

pub fn context(req: Request, path: &str) {
    if extension(path) != "docx" {
        return json_error(req, 415, "El chat de WordCraft admite documentos DOCX.");
    }
    let Some(full) = safe_path(path) else {
        return json_error(req, 400, "Ruta no válida");
    };
    const STALE: &str =
        "El documento cambió en otra sesión. Vuelve a abrirlo para usar esa versión.";
    let expected = server::header(&req, "If-Match");
    if word_index_current(&full) {
        if let (Ok(tag), Ok(mut text)) = (
            fs::read_to_string(full.join(WORD_REVISION)),
            fs::read_to_string(full.join(WORD_TEXT)),
        ) {
            if expected.is_some_and(|expected| expected != tag) {
                return json_error(req, 412, STALE);
            }
            let truncated = text.len() > 160_000;
            if truncated {
                text.truncate(text.floor_char_boundary(160_000));
            }
            return server::json(
                req,
                json!({"path":path,"content":text,"truncated":truncated}),
            );
        }
    }
    let bytes = match read_document(&document_path(&full)) {
        Ok(bytes) => bytes,
        Err((status, message)) => return json_error(req, status, message),
    };
    if expected.is_some_and(|tag| tag != revision(&bytes)) {
        return json_error(req, 412, STALE);
    }
    match read_docx_text(&bytes) {
        Ok((content, truncated)) => server::json(
            req,
            json!({ "path": path, "content": content, "truncated": truncated }),
        ),
        Err((status, message)) => json_error(req, status, message),
    }
}

pub(crate) fn read_docx_text(bytes: &[u8]) -> Result<(String, bool), (u16, &'static str)> {
    read_docx_text_with_limit(bytes, Some(160_000))
}

fn read_docx_text_with_limit(
    bytes: &[u8],
    limit: Option<usize>,
) -> Result<(String, bool), (u16, &'static str)> {
    const INVALID: (u16, &str) = (
        422,
        "No se pudo extraer texto del DOCX. Comprueba que se puede abrir sin contraseña.",
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| INVALID)?;
    let document = archive.by_name("word/document.xml").map_err(|_| INVALID)?;
    let xml = server::read_limited(document, 8 * 1024 * 1024).map_err(|error| {
        if error.kind() == ErrorKind::FileTooLarge {
            (
                413,
                "El texto del documento es demasiado grande para el chat.",
            )
        } else {
            INVALID
        }
    })?;
    let xml = std::str::from_utf8(&xml).map_err(|_| INVALID)?;
    docx_xml_text(xml, limit).map_err(|_| INVALID)
}

fn docx_xml_text(xml: &str, limit: Option<usize>) -> Result<(String, bool), quick_xml::Error> {
    let mut reader = NsReader::from_str(xml);
    let mut output = String::new();
    let mut in_text = false;
    loop {
        let (namespace, event) = reader.read_resolved_event()?;
        let word = matches!(namespace, ResolveResult::Bound(ns) if matches!(ns.as_ref(),
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main" | "http://purl.oclc.org/ooxml/wordprocessingml/main"));
        match event {
            Event::Start(tag) if word && tag.local_name().as_ref() == "t" => in_text = true,
            Event::End(tag) if word => match tag.local_name().as_ref() {
                "t" => in_text = false,
                "p" => output.push('\n'),
                _ => {}
            },
            Event::Empty(tag) if word => match tag.local_name().as_ref() {
                "tab" => output.push('\t'),
                "br" | "cr" => output.push('\n'),
                _ => {}
            },
            Event::Text(text) if in_text => output.push_str(&text.xml10_content()),
            Event::GeneralRef(reference) if in_text => {
                if let Some(value) = reference.resolve_char_ref()? {
                    output.push(value);
                } else if let Some(value) =
                    quick_xml::escape::resolve_xml_entity(reference.as_ref())
                {
                    output.push_str(value);
                }
            }
            Event::Eof => return Ok((output.trim_end().to_owned(), false)),
            _ => {}
        }
        if let Some(limit) = limit.filter(|limit| output.len() > *limit) {
            output.truncate(output.floor_char_boundary(limit));
            return Ok((output, true));
        }
    }
}

pub fn save(mut req: Request, path: &str) {
    let Some(full) = safe_path(path) else {
        return json_error(req, 400, "Ruta no válida");
    };
    let Some(expected) = server::header(&req, "If-Match").map(str::to_owned) else {
        return json_error(req, 428, "Falta la versión original del archivo");
    };
    let bytes = match server::read_limited(req.as_reader(), server::MAX_FILE) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::FileTooLarge => {
            return json_error(req, 413, "El archivo supera 32 MiB");
        }
        Err(_) => return json_error(req, 400, "No se pudo leer el archivo"),
    };
    if !valid_format(path, &bytes) {
        return json_error(req, 415, "Formato de documento no válido");
    }
    let current = match read_document(&document_path(&full)) {
        Ok(bytes) => bytes,
        Err((status, message)) => return json_error(req, status, message),
    };
    if current != bytes {
        if revision(&current) != expected {
            return json_error(
                req,
                412,
                "El archivo cambió en otra sesión. Descarga tu copia para conservar los cambios.",
            );
        }
        if extension(path) == "docx" && is_file_package(&full) {
            let _ = fs::remove_file(full.join(WORD_REVISION));
        }
        let written = write_document(&full, &bytes);
        if written.is_err() {
            return json_error(req, 500, "No se pudo guardar el archivo");
        }
        notify_document(&full);
    }
    if extension(path) == "docx" && (current != bytes || !word_index_current(&full)) {
        queue_text(&full);
    }
    server::json(req, json!({"revision": revision(&bytes)}));
}

fn read_document(path: &Path) -> Result<Vec<u8>, (u16, &'static str)> {
    if !path.is_file() {
        return Err((404, "Archivo no encontrado"));
    }
    let file = File::open(path).map_err(|_| (500, "No se pudo abrir el archivo"))?;
    server::read_limited(file, server::MAX_FILE).map_err(|error| {
        if error.kind() == ErrorKind::FileTooLarge {
            (413, "El archivo supera 32 MiB")
        } else {
            (500, "No se pudo leer el archivo")
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_saved_document_only_wakes_its_own_subscribers() {
        let first = tempfile::NamedTempFile::new().unwrap();
        let second = tempfile::NamedTempFile::new().unwrap();
        let first_key = std::fs::canonicalize(first.path()).unwrap();
        let one = super::Subscription::new(first_key.clone());
        let two = super::Subscription::new(first_key);
        let other = super::Subscription::new(std::fs::canonicalize(second.path()).unwrap());
        super::notify_document(first.path());
        assert_eq!(one.receiver.try_recv(), Ok(()));
        assert_eq!(two.receiver.try_recv(), Ok(()));
        assert_eq!(
            other.receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        );
        drop(other);
        assert!(!super::subscribers()
            .lock()
            .unwrap()
            .contains_key(&std::fs::canonicalize(second.path()).unwrap()));
    }
    use super::*;
    use std::io::Write;

    #[test]
    fn packages_keep_bytes_and_index_only_after_all_editors_leave() {
        let vault = tempfile::tempdir().unwrap();
        let doc = vault.path().join("Informe.docx");
        let original = include_bytes!("blank.docx");
        create_document(&doc, original).unwrap();
        assert!(is_word_package(&doc));
        assert_eq!(fs::read(document_path(&doc)).unwrap(), original);
        assert!(create_document(&doc, original).is_err());

        let now = Instant::now();
        {
            let mut jobs = jobs().lock().unwrap();
            let job = jobs.get_mut(&doc).unwrap();
            job.due = Some(now);
            for session in ["primera", "segunda"] {
                job.editors.insert(session.into(), now + EDITOR_LEASE);
            }
        }
        let writes = Mutex::new(());
        index_ready_words(&writes);
        assert!(!doc.join(WORD_TEXT).exists());
        jobs()
            .lock()
            .unwrap()
            .get_mut(&doc)
            .unwrap()
            .editors
            .remove("primera");
        index_ready_words(&writes);
        assert!(!doc.join(WORD_TEXT).exists());
        jobs()
            .lock()
            .unwrap()
            .get_mut(&doc)
            .unwrap()
            .editors
            .remove("segunda");
        index_ready_words(&writes);
        assert!(word_index_current(&doc));
        assert_eq!(
            fs::read_to_string(doc.join(WORD_REVISION)).unwrap(),
            revision(original)
        );

        let old = vault.path().join("Legado.docx");
        fs::write(&old, original).unwrap();
        install_package(&old, Some(original), true).unwrap();
        assert_eq!(fs::read(document_path(&old)).unwrap(), original);
        assert!(!old.join(".previous.docx").exists());
    }

    #[test]
    fn reads_visible_docx_text_with_paragraphs_and_escaped_characters() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:body><w:p><w:r><w:t>Proyecto &amp; revisión</w:t><w:tab/><w:t>&#x1F4C4;</w:t></w:r></w:p>
            <w:p><w:r><w:delText>Texto borrado</w:delText><w:t>Segunda línea</w:t></w:r></w:p></w:body>
            </w:document>"#;
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert_eq!(
            read_docx_text(&bytes),
            Ok(("Proyecto & revisión\t📄\nSegunda línea".into(), false))
        );
        assert!(read_docx_text(b"not a docx").is_err());
    }
}

const CRAFT_JS: &str = include_str!("ui/craft-editor.js");
const CRAFT_MERGE_JS: &str = include_str!("ui/craft-merge.bundle.js");

pub(crate) fn handle(
    req: Request,
    method: Method,
    path: &str,
    query: &str,
    config: &server::Config,
) {
    match (method, path) {
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
        (Method::Get, path) if path.starts_with("/craft/") => {
            asset(req, &config.craft_root, &path["/craft/".len()..]);
        }
        (Method::Get, "/api/craft/file") => open(req, &query_param(query, "path")),
        (Method::Get, "/api/craft/context") => context(req, &query_param(query, "path")),
        (Method::Put, "/api/craft/file") => save(req, &query_param(query, "path")),
        (Method::Post, "/api/craft/presence") => editor_presence(
            req,
            &query_param(query, "path"),
            &query_param(query, "session"),
            query_param(query, "leave") == "true",
        ),
        _ => json_error(req, 404, "Ruta no encontrada"),
    }
}
