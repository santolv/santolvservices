//! A separate, loopback-only static listener. The API router never serves HTML.
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::AsyncReadExt;

#[derive(Clone)]
pub struct Frontend {
    file: Arc<PathBuf>,
    authority: String,
    api_origin: String,
}
impl Frontend {
    pub async fn new(
        file: &Path,
        origin: &str,
        api_origin: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let origin = url::Url::parse(origin)?;
        if origin.scheme() != "http"
            || origin.host_str() != Some("localhost")
            || origin.port().is_none()
        {
            return Err("static frontend requires an explicit HTTP localhost port".into());
        }
        let mut api = url::Url::parse(api_origin)?;
        if api.scheme() != "http"
            || !matches!(api.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        {
            return Err("local cookie frontend requires an HTTP loopback API; public HTTPS needs its own deployment configuration".into());
        }
        // localhost and 127.0.0.1 are different cookie sites, even on the same machine.
        api.set_host(Some("localhost"))?;
        let file = tokio::fs::canonicalize(file).await?;
        if file.extension().and_then(|v| v.to_str()) != Some("html") {
            return Err("frontend must be an HTML file".into());
        }
        let s = Self {
            file: Arc::new(file),
            authority: origin
                .as_str()
                .trim_start_matches("http://")
                .trim_end_matches('/')
                .into(),
            api_origin: api.origin().ascii_serialization(),
        };
        s.read().await?;
        Ok(s)
    }
    async fn read(&self) -> Result<String, std::io::Error> {
        let file = tokio::fs::File::open(&*self.file).await?;
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1).read_to_end(&mut bytes).await?;
        if bytes.len() > 1024 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTML exceeds 1 MiB",
            ));
        }
        String::from_utf8(bytes)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "HTML must be UTF-8"))
    }
}
pub fn router(s: Frontend) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .with_state(s)
}
async fn index(State(s): State<Frontend>, headers: HeaderMap) -> Response {
    if headers.get_all("host").iter().count() != 1
        || !headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case(&s.authority))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let html = match s.read().await {
        Ok(v) => v,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let nonce = crate::auth::token();
    let html = html.replace(
        "const API_ORIGIN=location.protocol==='https:'?location.origin:'http://localhost:8443';",
        &format!(
            "const API_ORIGIN=location.protocol==='https:'?location.origin:{};",
            serde_json::to_string(&s.api_origin).expect("origin serialization")
        ),
    );
    let html = html.replace("<script>", &format!("<script nonce=\"{nonce}\">"));
    let mut response = Response::new(Body::from(html));
    for (key, value) in [
        ("content-type", "text/html; charset=utf-8".to_owned()),
        ("cache-control", "no-store".into()),
        ("x-content-type-options", "nosniff".into()),
        ("referrer-policy", "no-referrer".into()),
        ("x-frame-options", "DENY".into()),
        ("cross-origin-resource-policy", "same-origin".into()),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()".into(),
        ),
        (
            "content-security-policy",
            format!(
                "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'unsafe-inline'; connect-src {}; img-src 'self' blob:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'",
                s.api_origin
            ),
        ),
    ] {
        response
            .headers_mut()
            .insert(key, value.parse().expect("validated header"));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn only_the_selected_html_is_public_and_active_scripts_need_a_nonce() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("index.html");
        tokio::fs::write(&file, "<!doctype html><script>window.example=1</script>")
            .await
            .unwrap();
        let app = router(
            Frontend::new(&file, "http://localhost:5500", "http://127.0.0.1:8443")
                .await
                .unwrap(),
        );
        let req = |path: &str, host: &str| {
            axum::http::Request::builder()
                .uri(path)
                .header("host", host)
                .body(Body::empty())
                .unwrap()
        };
        let response = app
            .clone()
            .oneshot(req("/", "localhost:5500"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let csp = response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(csp.contains("connect-src http://localhost:8443"));
        assert!(!csp.contains("script-src 'unsafe-inline'"));
        assert!(!response.headers().contains_key("set-cookie"));
        let body = String::from_utf8(
            axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let nonce = body
            .split("nonce=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert!(csp.contains(nonce));
        for path in ["/api/v1/me", "/server.json", "/../server.json"] {
            assert_eq!(
                app.clone()
                    .oneshot(req(path, "localhost:5500"))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
        assert_eq!(
            app.oneshot(req("/", "attacker.example:5500"))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}
