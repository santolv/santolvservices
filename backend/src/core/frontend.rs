//! A separate, loopback-only static listener. The API router never serves HTML.
use axum::{
    Router,
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use std::{path::Path, sync::Arc};
use tokio::io::AsyncReadExt;

#[derive(Clone)]
pub struct Frontend {
    html: Arc<str>,
    modules: Vec<(crate::core::manager::Service, Arc<str>)>,
    db: Option<sqlx::PgPool>,
    authority: String,
    api_origin: String,
}
impl Frontend {
    pub async fn new(
        file: &Path,
        origin: &str,
        api_origin: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let file = tokio::fs::canonicalize(file).await?;
        if file.extension().and_then(|v| v.to_str()) != Some("html") {
            return Err("frontend must be an HTML file".into());
        }
        Self::from_html(Self::read(&file).await?.into(), origin, api_origin)
    }
    fn from_html(
        html: Arc<str>,
        origin: &str,
        api_origin: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let origin = url::Url::parse(origin)?;
        if origin.scheme() != "http"
            || origin.host_str() != Some("localhost")
            || origin.port().is_none()
            || origin.as_str().trim_end_matches('/') != origin.origin().ascii_serialization()
        {
            return Err(
                "static frontend requires a canonical HTTP localhost origin with explicit port"
                    .into(),
            );
        }
        let mut api = url::Url::parse(api_origin)?;
        if api.scheme() != "http"
            || !matches!(api.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        {
            return Err("local cookie frontend requires an HTTP loopback API; public HTTPS needs its own deployment configuration".into());
        }
        api.set_host(Some("localhost"))?;
        Ok(Self {
            html,
            modules: Vec::new(),
            db: None,
            authority: origin
                .as_str()
                .trim_start_matches("http://")
                .trim_end_matches('/')
                .into(),
            api_origin: api.origin().ascii_serialization(),
        })
    }
    pub fn bundled(origin: &str, api_origin: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut s = Self::from_html(
            Self::with_shell(include_str!("../../../frontend/home/index.html")),
            origin,
            api_origin,
        )?;
        s.modules = crate::modules::REGISTERED
            .iter()
            .map(|service| (*service, Self::with_shell(service.html)))
            .collect();
        Ok(s)
    }
    pub fn with_services(mut self, db: sqlx::PgPool) -> Self {
        self.db = Some(db);
        self
    }
    fn with_shell(html: &str) -> Arc<str> {
        html.replace(
            "<!-- SANTOLV_SHELL -->",
            include_str!("../../../frontend/shared/shell.html"),
        )
        .replacen(
            "<head>",
            &format!(
                "<head><style>{}</style><script>{}</script>",
                include_str!("../../../frontend/shared/shell.css"),
                include_str!("../../../frontend/shared/shell.js")
            ),
            1,
        )
        .into()
    }
    async fn read(path: &Path) -> Result<String, std::io::Error> {
        let file = tokio::fs::File::open(path).await?;
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
    let mut router = Router::new()
        .route("/", get(index))
        .route("/index.html", get(index));
    for (service, _) in &s.modules {
        for path in [
            service.page.to_owned(),
            format!("{}/", service.page),
            format!("{}/index.html", service.page),
        ] {
            router = router.route(&path, get(index));
        }
    }
    router.with_state(s)
}

async fn index(
    State(s): State<Frontend>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Response {
    if headers.get_all("host").iter().count() != 1
        || !headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case(&s.authority))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let module = s.modules.iter().find(|(service, _)| {
        uri.path() == service.page
            || uri
                .path()
                .strip_prefix(service.page)
                .is_some_and(|suffix| matches!(suffix, "/" | "/index.html"))
    });
    let mut html = module.map_or_else(|| s.html.to_string(), |(_, html)| html.to_string());
    if let Some(db) = &s.db {
        if let Some((service, _)) = module {
            match crate::core::manager::enabled(db, service.id).await {
                Ok(true) => {}
                Ok(false) => return crate::core::manager::disabled().into_response(),
                Err(error) => return error.into_response(),
            }
        } else {
            for (service, _) in &s.modules {
                match crate::core::manager::enabled(db, service.id).await {
                    Ok(true) => {}
                    Ok(false) => {
                        html = html.replace(
                            &format!("class=\"service\" href=\"{}/\"", service.page),
                            &format!("hidden inert class=\"service\" href=\"{}/\"", service.page),
                        );
                    }
                    Err(error) => return error.into_response(),
                }
            }
        }
    }
    let nonce = crate::auth::token();
    let html = html.replace(
        "const API_ORIGIN=location.protocol==='https:'?location.origin:'http://localhost:8443';",
        &format!(
            "const API_ORIGIN=location.protocol==='https:'?location.origin:{};",
            serde_json::to_string(&s.api_origin).expect("origin serialization")
        ),
    );
    let html = html.replace("<script>", &format!("<script nonce=\"{nonce}\">"));
    let html = html.replace("<style>", &format!("<style nonce=\"{nonce}\">"));
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
                "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; style-src-attr 'unsafe-inline'; connect-src {}; img-src 'self' blob:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'",
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
    async fn services_serve_separate_documents_without_exposing_source_files() {
        let app =
            router(Frontend::bundled("http://localhost:5500", "http://127.0.0.1:8443").unwrap());
        for (path, expected, absent) in [
            ("/", "Elige una app", "id=\"navigation\""),
            ("/drive/", "SantolvDrive", "id=\"periods\""),
            ("/photos/index.html", "SantolvPhotos", "id=\"navigation\""),
        ] {
            let response = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri(path)
                        .header("host", "localhost:5500")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(
                response.headers()["content-security-policy"]
                    .to_str()
                    .unwrap()
                    .contains("script-src 'nonce-")
            );
            let body = String::from_utf8(
                axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert!(body.contains(expected), "{path}");
            assert!(!body.contains(absent), "{path}");
            assert!(body.contains("window.SantolvShell"), "{path}");
            assert!(!body.contains("<script>"), "{path}: unprotected script");
        }
        for path in [
            "/server.json",
            "/photos/AGENTS.md",
            "/drive/../../server.json",
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(
                        axum::http::Request::builder()
                            .uri(path)
                            .header("host", "localhost:5500")
                            .body(Body::empty())
                            .unwrap()
                    )
                    .await
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
    }
    #[tokio::test]
    async fn only_the_selected_html_is_public_and_active_scripts_need_a_nonce() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("index.html");
        tokio::fs::write(
            &file,
            "<!doctype html><style>body{color:black}</style><script>window.example=1</script>",
        )
        .await
        .unwrap();
        let app = router(
            Frontend::new(&file, "http://localhost:5500", "http://127.0.0.1:8443")
                .await
                .unwrap(),
        );
        tokio::fs::write(&file, "<script>modified_after_startup()</script>")
            .await
            .unwrap();
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
        assert!(!csp.contains("style-src 'unsafe-inline'"));
        assert!(!response.headers().contains_key("set-cookie"));
        let body = String::from_utf8(
            axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(body.contains("window.example=1"));
        assert!(!body.contains("modified_after_startup"));
        let nonce = body
            .split("nonce=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert!(csp.contains(nonce));
        assert!(body.contains(&format!("<style nonce=\"{nonce}\">")));
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
