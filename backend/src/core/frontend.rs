//! A separate, loopback-only static listener. The API router never serves HTML.
use axum::{
    Router,
    body::Body,
    extract::{FromRequestParts, Request, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct Frontend {
    html: Arc<str>,
    account: Arc<str>,
    modules: Vec<(crate::core::manager::Service, Arc<str>)>,
    app: crate::App,
    authority: String,
    api_origin: String,
}
impl Frontend {
    pub fn bundled(
        origin: &str,
        api_origin: &str,
        app: crate::App,
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
            html: Self::with_shell(include_str!("../../../frontend/home/index.html")),
            account: Self::with_shell(include_str!("../../../frontend/account/index.html")),
            modules: crate::modules::REGISTERED
                .iter()
                .map(|service| (*service, Self::with_shell(service.html)))
                .collect(),
            app,
            authority: origin
                .as_str()
                .trim_start_matches("http://")
                .trim_end_matches('/')
                .into(),
            api_origin: api.origin().ascii_serialization(),
        })
    }
    fn with_shell(html: &str) -> Arc<str> {
        let sidebar = include_str!("../../../frontend/shared/sidebar-button.html").trim();
        let upload = include_str!("../../../frontend/shared/upload-button.html").trim();
        html.replace(
            "<!-- SANTOLV_RECORD_STYLE -->",
            concat!(
                "<style>",
                include_str!("../../../frontend/shared/records.css"),
                "</style>"
            ),
        )
        .replace(
            "<!-- SANTOLV_RECORD_SCRIPT -->",
            concat!(
                "<script>",
                include_str!("../../../frontend/shared/records.js"),
                "</script>"
            ),
        )
        .replace(
            "<!-- SANTOLV_SHELL -->",
            include_str!("../../../frontend/shared/shell.html"),
        )
        .replace(
            "<!-- SANTOLV_UPLOAD_FILE -->",
            &upload
                .replace("{{action}}", "upload")
                .replace("{{label}}", "Subir archivo"),
        )
        .replace(
            "<!-- SANTOLV_UPLOAD_PHOTO -->",
            &upload
                .replace("{{action}}", "import")
                .replace("{{label}}", "Subir fotos"),
        )
        .replace(
            "<!-- SANTOLV_SIDEBAR_HIDE -->",
            &sidebar
                .replace("{{action}}", "hide")
                .replace("{{label}}", "Ocultar barra lateral"),
        )
        .replace(
            "<!-- SANTOLV_SIDEBAR_SHOW -->",
            &sidebar
                .replace("{{action}}", "show")
                .replace("{{label}}", "Mostrar barra lateral"),
        )
        .replacen(
            "<head>",
            &format!(
                "<head><style>{}</style><script>{}</script><script>{}</script>",
                include_str!("../../../frontend/shared/shell.css"),
                include_str!("../../../frontend/shared/calendar.js"),
                include_str!("../../../frontend/shared/shell.js")
            ),
            1,
        )
        .into()
    }
}
pub fn router(s: Frontend) -> Router {
    let mut router = Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/account", get(index))
        .route("/account/", get(index));
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

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

async fn index(State(s): State<Frontend>, request: Request) -> Response {
    let (mut parts, _) = request.into_parts();
    let headers = &parts.headers;
    if headers.get_all("host").iter().count() != 1
        || !headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case(&s.authority))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let module = s.modules.iter().find(|(service, _)| {
        parts.uri.path() == service.page
            || parts
                .uri
                .path()
                .strip_prefix(service.page)
                .is_some_and(|suffix| matches!(suffix, "/" | "/index.html"))
    });
    // Browser documents use the cookie, never a native client's bearer token.
    parts.headers.remove("authorization");
    let user = match crate::auth::User::from_request_parts(&mut parts, &s.app).await {
        Ok(user) => user,
        Err(error) if matches!(error.0, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) => {
            return if parts.uri.path() != "/" {
                no_store(Redirect::to("/").into_response())
            } else {
                render(&s, include_str!("../../../frontend/home/login.html"), true)
            };
        }
        Err(error) => return no_store(error.into_response()),
    };
    if let Some((service, _)) = module
        && let Err(error) =
            crate::core::manager::require_access(&s.app.db, service.id, user.id).await
    {
        return no_store(error.into_response());
    }
    let allowed = match crate::core::manager::accessible(&s.app.db, user.id).await {
        Ok(allowed) => allowed,
        Err(error) => return no_store(error.into_response()),
    };
    let template = if matches!(parts.uri.path(), "/account" | "/account/") {
        &s.account
    } else {
        module.map_or(&s.html, |(_, html)| html)
    };
    // Each service link is on its own line in the templates.
    let html = template
        .lines()
        .filter(|line| {
            !s.modules.iter().any(|(service, _)| {
                !allowed.iter().any(|id| id == service.id)
                    && (line.contains(&format!("data-shell-service=\"{}\"", service.id))
                        || line.contains(&format!("class=\"service\" href=\"{}/\"", service.page)))
            })
        })
        .collect::<Vec<_>>()
        .join("\n");
    render(&s, &html, false)
}

fn render(s: &Frontend, html: &str, login: bool) -> Response {
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
                "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; style-src-attr {}; connect-src {}; img-src 'self' blob:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'",
                if login { "'none'" } else { "'unsafe-inline'" },
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
