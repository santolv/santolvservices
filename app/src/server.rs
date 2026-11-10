use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use subtle::ConstantTimeEq;
use tiny_http::{Header, Request, Response};
use url::Url;

pub const MAX_FILE: u64 = 32 * 1024 * 1024;

pub struct Config {
    pub address: SocketAddr,
    authorization: Option<String>,
    pub tools: Vec<ToolLink>,
    pub craft_root: PathBuf,
}

#[derive(Deserialize, Serialize)]
pub struct ToolLink {
    name: String,
    url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let host: IpAddr = env("SUPER_CEREBRO_HOST")
            .unwrap_or_else(|| "127.0.0.1".into())
            .parse()
            .map_err(|_| "SUPER_CEREBRO_HOST debe ser una dirección IP")?;
        let port = env("SUPER_CEREBRO_PORT")
            .unwrap_or_else(|| "7878".into())
            .parse()
            .map_err(|_| "SUPER_CEREBRO_PORT no es válido")?;
        let authorization = match (env("SUPER_CEREBRO_USER"), env("SUPER_CEREBRO_PASSWORD")) {
            (Some(user), Some(password)) if !user.contains(':') => {
                Some(format!("Basic {}", STANDARD.encode(format!("{user}:{password}"))))
            }
            (None, None) if host.is_loopback() => None,
            _ => return Err("Configura SUPER_CEREBRO_USER y SUPER_CEREBRO_PASSWORD. Son obligatorios fuera de localhost.".into()),
        };
        let tools: Vec<ToolLink> =
            serde_json::from_str(&env("SUPER_CEREBRO_TOOLS").unwrap_or_else(|| "[]".into()))
                .map_err(|_| "SUPER_CEREBRO_TOOLS debe ser una lista JSON de herramientas")?;
        for tool in &tools {
            if tool.name.trim().is_empty() || http_url(&tool.url).is_none() {
                return Err("Cada herramienta necesita un nombre y una URL HTTP(S)".into());
            }
        }
        Ok(Self {
            address: SocketAddr::new(host, port),
            authorization,
            tools,
            craft_root: std::env::var_os("SUPER_CEREBRO_CRAFT_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("craft-apps")),
        })
    }

    pub fn authorized(&self, req: &Request) -> bool {
        self.authorization.as_ref().is_none_or(|expected| {
            header(req, "Authorization")
                .is_some_and(|value| bool::from(value.as_bytes().ct_eq(expected.as_bytes())))
        })
    }
}

pub fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn http_url(value: &str) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.has_host()
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

pub fn header<'a>(req: &'a Request, name: &'static str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str())
}

pub fn trusted_origin(req: &Request) -> bool {
    if header(req, "Sec-Fetch-Site") == Some("cross-site") {
        return false;
    }
    let Some(origin) = header(req, "Origin") else {
        return true;
    };
    let Some(origin) = http_url(origin) else {
        return false;
    };
    let Some(host) = header(req, "Host") else {
        return false;
    };
    http_url(&format!("{}://{host}", origin.scheme()))
        .is_some_and(|host| host.origin() == origin.origin())
}

pub fn unauthorized(req: Request) {
    let _ = req.respond(
        Response::from_string("Acceso a Super Cerebro")
            .with_status_code(401)
            .with_header(
                Header::from_bytes(
                    "WWW-Authenticate",
                    "Basic realm=\"Super Cerebro\", charset=\"UTF-8\"",
                )
                .unwrap(),
            ),
    );
}

pub fn json(req: Request, value: serde_json::Value) {
    let _ = req.respond(
        Response::from_string(value.to_string())
            .with_header(
                Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap(),
            )
            .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap()),
    );
}

pub fn read_limited(reader: impl Read, limit: u64) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    reader.take(limit + 1).read_to_end(&mut buffer)?;
    if buffer.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "Archivo demasiado grande",
        ));
    }
    Ok(buffer)
}

pub fn read_json<T: serde::de::DeserializeOwned>(req: &mut Request, limit: u64) -> Result<T, u16> {
    let bytes = read_limited(req.as_reader(), limit).map_err(|error| {
        if error.kind() == io::ErrorKind::FileTooLarge {
            413u16
        } else {
            400u16
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|_| 400)
}

pub fn write_atomic(path: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::Builder::new()
        .prefix(".sc-")
        .tempfile_in(parent)?;
    if replace {
        if let Ok(metadata) = fs::metadata(path) {
            file.as_file().set_permissions(metadata.permissions())?;
        }
    }
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    if replace {
        file.persist(path)?;
    } else {
        file.persist_noclobber(path)?;
    }
    Ok(())
}
