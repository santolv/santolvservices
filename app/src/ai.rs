use crate::{json_error, server};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use std::{fs, path::PathBuf};
use tiny_http::Request;

pub struct Settings {
    file: PathBuf,
    current: Mutex<Option<Connection>>,
    agent: ureq::Agent,
    active: AtomicUsize,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Provider {
    Opencode,
    Openai,
    Compatible,
}

#[derive(Clone, Deserialize, Serialize)]
struct Credentials {
    provider: Provider,
    base_url: String,
    model: String,
    #[serde(default)]
    key: String,
}

#[derive(Clone)]
struct Connection {
    credentials: Credentials,
    endpoint: url::Url,
}

#[derive(Deserialize)]
struct ChatInput {
    message: String,
}

const OPENCODE_URL: &str = "https://opencode.ai/inference/openai/v1";
const OPENAI_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "deepseek-v4.1-flash";

impl Credentials {
    fn from_env() -> Option<Self> {
        let key =
            server::env("SUPER_CEREBRO_AI_KEY").or_else(|| server::env("OPENCODE_API_KEY"))?;
        let base_url =
            server::env("SUPER_CEREBRO_AI_BASE_URL").unwrap_or_else(|| OPENCODE_URL.into());
        let provider = if base_url.trim_end_matches('/') == OPENCODE_URL {
            Provider::Opencode
        } else {
            Provider::Compatible
        };
        let model = server::env("SUPER_CEREBRO_AI_MODEL").unwrap_or_else(|| DEFAULT_MODEL.into());
        Some(Self {
            provider,
            base_url,
            model,
            key,
        })
    }

    fn validate(mut self) -> Result<Connection, &'static str> {
        let base = match self.provider {
            Provider::Opencode => OPENCODE_URL,
            Provider::Openai => OPENAI_URL,
            Provider::Compatible => self.base_url.trim().trim_end_matches('/'),
        };
        let endpoint = server::http_url(&format!("{base}/chat/completions"))
            .filter(|url| url.query().is_none() && url.fragment().is_none())
            .filter(|url| {
                url.scheme() == "https"
                    || url.host_str() == Some("localhost")
                    || url
                        .host_str()
                        .and_then(|host| {
                            host.trim_matches(['[', ']'])
                                .parse::<std::net::IpAddr>()
                                .ok()
                        })
                        .is_some_and(|ip| ip.is_loopback())
            })
            .ok_or("La URL de IA debe usar HTTPS o apuntar a localhost")?;
        if base.len() > 2048 {
            return Err("Introduce una URL base válida");
        }
        self.base_url = base.to_owned();
        self.model = self.model.trim().to_owned();
        self.key = self.key.trim().to_owned();
        if self.model.is_empty()
            || self.model.len() > 128
            || self.model.chars().any(char::is_control)
        {
            return Err("Introduce un identificador de modelo válido");
        }
        if self.key.len() > 4096 || !self.key.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err("La clave API contiene caracteres no válidos");
        }
        Ok(Connection {
            credentials: self,
            endpoint,
        })
    }
}

impl Settings {
    pub fn load(file: PathBuf) -> Result<Arc<Self>, String> {
        if crate::linked_path(&file) {
            return Err("La configuración de IA no puede ser un enlace".into());
        }
        let current = match fs::File::open(&file) {
            Ok(file) => {
                let bytes = server::read_limited(file, 16 * 1024)
                    .map_err(|_| "No se pudo leer la configuración de IA")?;
                serde_json::from_slice::<Option<Credentials>>(&bytes)
                    .map_err(|_| "La configuración de IA no es válida")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Credentials::from_env(),
            Err(_) => return Err("No se pudo abrir la configuración de IA".into()),
        };
        let agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(75)))
            .build()
            .into();
        Ok(Arc::new(Self {
            file,
            current: Mutex::new(current.map(Credentials::validate).transpose()?),
            agent,
            active: AtomicUsize::new(0),
        }))
    }

    pub fn status(&self, req: Request) {
        let model = self
            .current
            .lock()
            .unwrap()
            .as_ref()
            .map(|chat| chat.credentials.model.clone());
        server::json(req, json!({"enabled": model.is_some(), "model": model}));
    }

    pub fn edit(&self, mut req: Request) {
        if req.method() == &tiny_http::Method::Get {
            let settings = self.current.lock().unwrap().as_ref().map_or_else(
                || json!({"provider": "opencode", "base_url": OPENCODE_URL, "model": DEFAULT_MODEL, "has_key": false}),
                |chat| json!({"provider": chat.credentials.provider, "base_url": chat.credentials.base_url, "model": chat.credentials.model, "has_key": true}),
            );
            return server::json(req, settings);
        }
        let mut next = if req.method() == &tiny_http::Method::Delete {
            None
        } else {
            let input = match server::read_json::<Credentials>(&mut req, 16 * 1024) {
                Ok(input) => input,
                Err(status) => return json_error(req, status, "Ajustes de IA no válidos"),
            };
            let connection = match input.validate() {
                Ok(connection) => connection,
                Err(error) => return json_error(req, 400, error),
            };
            Some(connection)
        };
        let mut current = self.current.lock().unwrap();
        if let Some(connection) = next.as_mut().filter(|chat| chat.credentials.key.is_empty()) {
            let Some(previous) = current
                .as_ref()
                .filter(|old| old.endpoint == connection.endpoint)
            else {
                return json_error(req, 400, "Introduce la clave API de este proveedor");
            };
            connection
                .credentials
                .key
                .clone_from(&previous.credentials.key);
        }
        let bytes = serde_json::to_vec(&next.as_ref().map(|chat| &chat.credentials)).unwrap();
        if crate::linked_path(&self.file) || server::write_atomic(&self.file, &bytes, true).is_err()
        {
            return json_error(req, 500, "No se pudieron guardar los ajustes de IA");
        }
        *current = next;
        drop(current);
        server::json(req, json!({"ok": true}));
    }

    pub fn send(self: &Arc<Self>, mut req: Request) {
        let Some(connection) = self.current.lock().unwrap().clone() else {
            return json_error(req, 503, "Configura la IA en Mis archivos → Ajustes");
        };
        let input = match server::read_json::<ChatInput>(&mut req, 64 * 1024) {
            Ok(input)
                if !input.message.trim().is_empty() && input.message.chars().count() <= 8000 =>
            {
                input
            }
            Ok(_) => return json_error(req, 400, "Escribe un mensaje de hasta 8000 caracteres"),
            Err(status) => return json_error(req, status, "Solicitud de IA no válida"),
        };
        if self.active.fetch_add(1, Ordering::Relaxed) >= 2 {
            self.active.fetch_sub(1, Ordering::Relaxed);
            return json_error(req, 429, "La IA está ocupada. Inténtalo en unos segundos.");
        }
        let chat = Arc::clone(self);
        std::thread::spawn(move || {
            struct Slot<'a>(&'a AtomicUsize);
            impl Drop for Slot<'_> {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::Relaxed);
                }
            }
            let _slot = Slot(&chat.active);
            match chat.complete(connection, input.message) {
                Ok(text) => server::json(req, json!({"content": text})),
                Err((code, message)) => json_error(req, code, message),
            }
        });
    }

    fn complete(
        &self,
        connection: Connection,
        message: String,
    ) -> Result<String, (u16, &'static str)> {
        let invalid = (502, "El proveedor de IA devolvió una respuesta no válida");
        let free_opencode = matches!(connection.credentials.provider, Provider::Opencode)
            && connection.credentials.model.ends_with("-free");
        let mut payload = json!({
            "model": connection.credentials.model,
            "messages": [{"role": "user", "content": message}],
        });
        let limit = match connection.credentials.provider {
            Provider::Openai => "max_completion_tokens",
            _ => "max_tokens",
        };
        payload[limit] = json!(2048);
        let mut response = self
            .agent
            .post(connection.endpoint.as_str())
            .header(
                "Authorization",
                format!("Bearer {}", connection.credentials.key),
            )
            .send_json(payload)
            .map_err(|error| match error {
                ureq::Error::StatusCode(403) if free_opencode => (
                    502, "OpenCode reserva este modelo gratuito para su cliente. Elige un modelo habilitado para la API en Mis archivos → Ajustes.",
                ),
                ureq::Error::StatusCode(401 | 403) => {
                    (502, "El proveedor rechazó la clave o el modelo")
                }
                ureq::Error::StatusCode(429) => {
                    (429, "El proveedor de IA alcanzó su límite de uso")
                }
                ureq::Error::Timeout(_) => (504, "El proveedor de IA tardó demasiado"),
                _ => (502, "No se pudo conectar con el proveedor de IA"),
            })?;
        if !response.status().is_success() {
            return Err(invalid);
        }
        let body: Value = response
            .body_mut()
            .with_config()
            .limit(512 * 1024)
            .read_json()
            .map_err(|_| invalid)?;
        body.pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned)
            .ok_or(invalid)
    }
}
