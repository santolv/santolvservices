use crate::{json_error, server};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tiny_http::Request;

pub struct Chat {
    endpoint: url::Url,
    key: String,
    model: String,
    agent: ureq::Agent,
    active: AtomicUsize,
}

#[derive(Deserialize)]
struct Turn {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct Conversation {
    messages: Vec<Turn>,
}

impl Chat {
    pub fn from_env() -> Result<Option<Arc<Self>>, String> {
        let Some(key) =
            server::env("SUPER_CEREBRO_AI_KEY").or_else(|| server::env("OPENCODE_API_KEY"))
        else {
            return Ok(None);
        };
        let base = server::env("SUPER_CEREBRO_AI_BASE_URL")
            .unwrap_or_else(|| "https://opencode.ai/inference/openai/v1".into());
        let endpoint =
            server::http_url(&format!("{}/chat/completions", base.trim_end_matches('/')))
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
        let model =
            server::env("SUPER_CEREBRO_AI_MODEL").unwrap_or_else(|| "deepseek-v4.1-flash".into());
        let agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(75)))
            .build()
            .into();
        Ok(Some(Arc::new(Self {
            endpoint,
            key,
            model,
            agent,
            active: AtomicUsize::new(0),
        })))
    }

    pub fn status(chat: Option<&Self>, req: Request) {
        server::json(
            req,
            json!({"enabled": chat.is_some(), "model": chat.map(|chat| &chat.model)}),
        );
    }

    pub fn send(chat: Option<&Arc<Self>>, mut req: Request) {
        let Some(chat) = chat else {
            return json_error(req, 503, "Configura la clave de IA en el servidor");
        };
        let input = match server::read_json::<Conversation>(&mut req, 192 * 1024) {
            Ok(input) if input.valid() => input,
            Ok(_) => return json_error(req, 400, "Conversación no válida"),
            Err(status) => return json_error(req, status, "Solicitud de IA no válida"),
        };
        if chat.active.fetch_add(1, Ordering::Relaxed) >= 2 {
            chat.active.fetch_sub(1, Ordering::Relaxed);
            return json_error(req, 429, "La IA está ocupada. Inténtalo en unos segundos.");
        }
        let chat = Arc::clone(chat);
        std::thread::spawn(move || {
            struct Slot<'a>(&'a AtomicUsize);
            impl Drop for Slot<'_> {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::Relaxed);
                }
            }
            let _slot = Slot(&chat.active);
            match chat.complete(input) {
                Ok(text) => server::json(req, json!({"content": text})),
                Err((code, message)) => json_error(req, code, message),
            }
        });
    }

    fn complete(&self, input: Conversation) -> Result<String, (u16, &'static str)> {
        let messages: Vec<_> = input
            .messages
            .into_iter()
            .map(|turn| json!({"role":turn.role,"content":turn.content}))
            .collect();
        let invalid = (502, "El proveedor de IA devolvió una respuesta no válida");
        let mut response = self
            .agent
            .post(self.endpoint.as_str())
            .header("Authorization", format!("Bearer {}", self.key))
            .send_json(json!({"model": self.model, "messages": messages, "max_tokens": 2048}))
            .map_err(|error| match error {
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

impl Conversation {
    fn valid(&self) -> bool {
        !self.messages.is_empty()
            && self.messages.len() <= 13
            && self.messages.last().is_some_and(|turn| turn.role == "user")
            && self.messages.iter().all(|turn| {
                matches!(turn.role.as_str(), "user" | "assistant")
                    && !turn.content.trim().is_empty()
            })
    }
}
