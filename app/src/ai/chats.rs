use crate::{accounts::Actor, craft, drive, json_error, server};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};
use tiny_http::{Method, Request};

const HISTORY: &str = ".chat-ia.json";
const LIMIT: u64 = 4 * 1024 * 1024;
type Failure = (u16, &'static str);

#[derive(Deserialize, Serialize)]
pub(super) struct Turn {
    pub author: String,
    pub message: String,
    pub reply: Reply,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reply {
    pub response: String,
    pub changes: Vec<Change>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Change {
    location: String,
    original: String,
    replacement: String,
}

impl Reply {
    pub fn parse(text: &str) -> Result<Self, Failure> {
        let reply: Self = serde_json::from_str(text)
            .map_err(|_| (502, "La IA no devolvió cambios en el formato solicitado"))?;
        if reply.response.trim().is_empty()
            || reply.changes.len() > 30
            || reply.changes.iter().any(|change| {
                change.location.trim().is_empty()
                    || (change.original.is_empty() && change.replacement.is_empty())
            })
        {
            return Err((502, "La IA devolvió una propuesta incompleta"));
        }
        Ok(reply)
    }
}

pub(super) fn history(path: &str) -> Result<PathBuf, Failure> {
    if path.is_empty() {
        return fs::canonicalize(drive::vault_root())
            .map(|root| root.join(HISTORY))
            .map_err(|_| (404, "Bóveda no disponible"));
    }
    let full = drive::safe_path(path).ok_or((404, "Documento no disponible"))?;
    if !drive::is_file_package(&full) {
        return Err((404, "Documento no disponible"));
    }
    fs::canonicalize(full)
        .map(|folder| folder.join(HISTORY))
        .map_err(|_| (404, "Documento no disponible"))
}

fn read(file: &PathBuf) -> Result<Vec<Turn>, Failure> {
    if drive::linked_path(file) {
        return Err((403, "Historial no disponible"));
    }
    match fs::File::open(file) {
        Ok(input) => server::read_limited(input, LIMIT)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or((500, "No se pudo leer el historial")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err((500, "No se pudo abrir el historial")),
    }
}

pub(super) fn append(file: &PathBuf, turn: Turn) -> Result<Vec<Turn>, Failure> {
    let mut turns = read(file)?;
    turns.push(turn);
    let data = serde_json::to_vec(&turns).map_err(|_| (500, "No se pudo preparar el historial"))?;
    if data.len() as u64 > LIMIT {
        return Err((
            413,
            "El historial alcanzó su límite. Puedes borrarlo desde Chats con IA.",
        ));
    }
    server::write_atomic(file, &data, true)
        .map_err(|_| (500, "No se pudo guardar el historial"))?;
    Ok(turns)
}

pub(super) fn document(path: &str) -> Result<String, Failure> {
    let full = drive::safe_path(path).ok_or((404, "Documento no disponible"))?;
    let payload = drive::document_path(&full);
    match full
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "docx" => {
            let file = fs::File::open(payload).map_err(|_| (404, "Documento no disponible"))?;
            let bytes = server::read_limited(file, server::MAX_FILE)
                .map_err(|_| (413, "El documento es demasiado grande para el chat"))?;
            let (text, truncated) = craft::read_docx_text(&bytes)?;
            if truncated {
                return Err((
                    413,
                    "El texto del documento es demasiado grande para el chat",
                ));
            }
            Ok(text)
        }
        "md" | "markdown" | "txt" | "base" | "canvas" => {
            let file = fs::File::open(payload).map_err(|_| (404, "Documento no disponible"))?;
            let bytes = server::read_limited(file, 160_000)
                .map_err(|_| (413, "El documento es demasiado grande para el chat"))?;
            String::from_utf8(bytes).map_err(|_| (415, "El documento debe estar en UTF-8"))
        }
        _ => Err((
            415,
            "El chat admite DOCX, notas y archivos de texto de Super Cerebro",
        )),
    }
}

pub(super) fn manage(req: Request, path: &str, pending: bool) {
    let result = history(path).and_then(|file| {
        if req.method() == &Method::Delete {
            if pending {
                return Err((409, "Espera a que termine la respuesta para borrar el chat"));
            }
            if drive::linked_path(&file) {
                return Err((403, "Historial no disponible"));
            }
            match fs::remove_file(file) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err((500, "No se pudo borrar el historial")),
            }
            Ok(Vec::new())
        } else {
            read(&file)
        }
    });
    match result {
        Ok(turns) => server::json(req, serde_json::json!({"turns": turns})),
        Err((status, error)) => json_error(req, status, error),
    }
}

pub(super) fn list(req: Request, config: &server::Config, actor: Option<&Actor>) {
    let mut paths = if let (Some(accounts), Some(actor)) = (&config.accounts, actor) {
        if !actor.owner {
            accounts
                .files(actor)
                .into_iter()
                .filter_map(|entry| entry["path"].as_str().map(str::to_owned))
                .collect()
        } else {
            vault_paths()
        }
    } else {
        vault_paths()
    };
    paths.retain(|path| {
        history(path).is_ok_and(|file| !drive::linked_path(&file) && file.is_file())
    });
    server::json(
        req,
        serde_json::json!({"paths": paths, "general": actor.is_none_or(|actor| actor.owner)}),
    );
}

fn vault_paths() -> Vec<String> {
    std::iter::once(String::new())
        .chain(
            drive::list_files()
                .into_iter()
                .filter(|entry| !entry.is_dir)
                .map(|entry| entry.path),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Reply;

    #[test]
    fn document_reply_requires_both_fields_and_valid_changes() {
        assert!(Reply::parse(r#"{"response":"Listo","changes":[]}"#).is_ok());
        assert!(Reply::parse(r#"{"response":"Listo"}"#).is_err());
        assert!(Reply::parse(r#"{"response":"Listo","changes":[{"location":"Título","original":"","replacement":""}]}"#).is_err());
    }
}
