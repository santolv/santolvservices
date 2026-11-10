use crate::{json_error, safe_path, safe_path_at, server};
use quick_xml::{events::Event, name::ResolveResult, NsReader};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Cursor, ErrorKind};
use std::path::Path;
use tiny_http::{Header, Request, Response};

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

pub fn open(req: Request, path: &str) {
    let Some(full) = safe_path(path) else {
        return json_error(req, 400, "Ruta no válida");
    };
    let bytes = match read_document(&full) {
        Ok(bytes) => bytes,
        Err((status, message)) => return json_error(req, status, message),
    };
    if !valid_format(path, &bytes) {
        return json_error(req, 415, "Formato de documento no válido");
    }
    let tag = revision(&bytes);
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
    let bytes = match read_document(&full) {
        Ok(bytes) => bytes,
        Err((status, message)) => return json_error(req, status, message),
    };
    if server::header(&req, "If-Match").is_some_and(|tag| tag != revision(&bytes)) {
        return json_error(
            req,
            412,
            "El documento cambió en otra sesión. Vuelve a abrirlo para usar esa versión.",
        );
    }
    let result = read_docx_text(&bytes);
    match result {
        Ok((content, truncated)) => server::json(
            req,
            json!({ "path": path, "content": content, "truncated": truncated }),
        ),
        Err((status, message)) => json_error(req, status, message),
    }
}

fn read_docx_text(bytes: &[u8]) -> Result<(String, bool), (u16, &'static str)> {
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
    docx_xml_text(xml).map_err(|_| INVALID)
}

fn docx_xml_text(xml: &str) -> Result<(String, bool), quick_xml::Error> {
    const LIMIT: usize = 160_000;
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
        if output.len() > LIMIT {
            output.truncate(output.floor_char_boundary(LIMIT));
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
    let current = match read_document(&full) {
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
        if server::write_atomic(&full, &bytes, true).is_err() {
            return json_error(req, 500, "No se pudo guardar el archivo");
        }
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
    use super::*;
    use std::io::Write;

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
