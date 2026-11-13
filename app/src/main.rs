use std::{fs, path::PathBuf, sync::Mutex};
use tiny_http::{Header, Method, Response, Server};
use urlencoding::decode;

mod accounts;
mod ai;
mod cerebro;
mod craft;
mod drive;
mod server;

use drive::{linked_path, safe_path, vault_root};

const REFERENCES_JS: &str = include_str!("ui/references.js");
const ACCOUNT_JS: &str = include_str!("ui/accounts.js");
const LOGIN_HTML: &str = include_str!("login.html");
const CHAT_JS: &str = include_str!("ui/chat.js");
const CHAT_CSS: &str = include_str!("ui/chat.css");
const STAGESHOW_CSS: &str = include_str!("ui/stageshow.css");
const APP_CSS: &str = include_str!("ui/app.css");

fn ui_asset(relative: &str, bundled: &str) -> String {
    // Debug builds pick up rebuilt assets on reload; release builds stay self-contained.
    if cfg!(debug_assertions) {
        if let Ok(asset) = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join(relative),
        ) {
            return asset;
        }
    }
    bundled.to_string()
}

fn query_param(query: &str, key: &str) -> String {
    for part in query.split('&') {
        if let Some((k, v)) = part.split_once('=') {
            if k == key {
                return decode(v).unwrap_or_default().to_string();
            }
        }
    }
    String::new()
}

fn json_error(req: tiny_http::Request, code: u16, error: &str) {
    let _ = req.respond(
        Response::from_string(serde_json::json!({"error": error}).to_string())
            .with_status_code(code)
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
    );
}

fn handle(
    req: tiny_http::Request,
    config: &std::sync::Arc<server::Config>,
    writes: &std::sync::Arc<Mutex<()>>,
) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));

    if method == Method::Get && path == "/healthz" {
        return server::json(req, serde_json::json!({"ok":true}));
    }
    let account = if let Some(accounts) = &config.accounts {
        if !accounts.host(&req) {
            return json_error(req, 403, "Host no permitido");
        }
        if matches!(
            method,
            Method::Post | Method::Put | Method::Delete | Method::Patch
        ) && !accounts.origin(&req)
        {
            return json_error(req, 403, "Origen de solicitud no permitido");
        }
        match (&method, path) {
            (&Method::Get, "/login") => {
                let response = Response::from_string(LOGIN_HTML)
                    .with_header(Header::from_bytes("Content-Type","text/html; charset=utf-8").unwrap())
                    .with_header(Header::from_bytes("Cache-Control","no-store").unwrap())
                    .with_header(Header::from_bytes("Content-Security-Policy","default-src 'none'; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'").unwrap());
                let _ = req.respond(response);
                return;
            }
            (&Method::Post, "/api/session/login") => return accounts.login(req),
            (&Method::Post, "/api/session/logout") => return accounts.logout(req),
            _ => {}
        }
        let actor = accounts.actor(&req);
        if actor.is_none() && !(method == Method::Get && path.starts_with("/ui/")) {
            if method == Method::Get
                && matches!(path, "/" | "/index.html" | "/cerebro" | "/cerebro/")
            {
                let next = format!("/login?next={}", urlencoding::encode(&url));
                return accounts::redirect(req, &next);
            }
            return json_error(req, 401, "Inicia sesión");
        }
        actor
    } else if !config.authorized(&req) {
        return server::unauthorized(req);
    } else {
        None
    };
    if method == Method::Get && path == "/api/craft/changes" {
        if !server::trusted_origin(&req) {
            return json_error(req, 403, "Origen de solicitud no permitido");
        }
        return craft::watch(
            req,
            query_param(query, "path"),
            config.clone(),
            writes.clone(),
        );
    }
    if path == "/api/ai/settings" && matches!(method, Method::Get | Method::Put | Method::Delete) {
        if !server::trusted_origin(&req)
            || !config.allows_ai_settings(&req)
            || account.as_ref().is_some_and(|actor| !actor.owner)
        {
            return json_error(
                req,
                403,
                "Solo el propietario puede cambiar los ajustes de IA",
            );
        }
        return config.ai.edit(req);
    }
    if matches!(
        (&method, path),
        (
            &Method::Get,
            "/api/ai/status" | "/api/ai/history" | "/api/ai/chats"
        ) | (&Method::Post, "/api/ai/chat")
            | (&Method::Delete, "/api/ai/history")
    ) {
        if !server::trusted_origin(&req) {
            return json_error(req, 403, "Origen de solicitud no permitido");
        }
        return ai::Settings::route(
            req,
            path,
            query_param(query, "path"),
            config.clone(),
            account,
            writes.clone(),
        );
    }
    let _guard = if matches!(method, Method::Post | Method::Put | Method::Delete)
        || matches!(
            path,
            "/api/craft/file"
                | "/api/craft/context"
                | "/api/download"
                | "/api/files"
                | "/api/craft/share"
                | "/api/file"
                | "/api/search"
                | "/api/backlinks"
                | "/api/outline"
                | "/api/properties"
                | "/api/graph"
        ) {
        if !server::trusted_origin(&req) {
            return json_error(req, 403, "Origen de solicitud no permitido");
        }
        Some(writes.lock().unwrap())
    } else {
        None
    };
    if let (Some(accounts), Some(actor)) = (&config.accounts, &account) {
        if !actor.owner {
            let office = matches!(
                (&method, path),
                (
                    &Method::Get,
                    "/api/craft/file" | "/api/craft/context" | "/api/download" | "/api/preview"
                ) | (&Method::Put, "/api/craft/file")
                    | (
                        &Method::Post,
                        "/api/craft/presence" | "/api/craft/participants"
                    )
            );
            let shell = method == Method::Get
                && (matches!(path, "/" | "/index.html" | "/api/hub" | "/api/files")
                    || path.starts_with("/ui/")
                    || ["wordcraft", "deckcraft", "gridcraft"]
                        .iter()
                        .any(|editor| path.starts_with(&format!("/craft/{editor}/"))));
            if !shell && !(office && accounts.allows(actor, &query_param(query, "path"))) {
                return json_error(req, 403, "Este archivo o acción no está compartido contigo");
            }
            if method == Method::Get && path == "/api/files" {
                return server::json(req, serde_json::json!(accounts.files(actor)));
            }
        }
        match (&method, path) {
            (&Method::Post, "/api/accounts") => return accounts.create_user(req, actor),
            (&Method::Get | &Method::Post, "/api/craft/share") => {
                return accounts.share(req, actor, query)
            }
            (&Method::Post, "/api/craft/participants")
                if accounts.allows(actor, &query_param(query, "path")) =>
            {
                return accounts.participants(req, actor, query)
            }
            _ => {}
        }
    }

    match (method, path) {
        (Method::Get, "/ui/references.js") => {
            respond_asset(
                req,
                "ui/references.js",
                REFERENCES_JS,
                "text/javascript; charset=utf-8",
            );
        }
        (Method::Get, "/ui/accounts.js") => {
            respond_asset(
                req,
                "ui/accounts.js",
                ACCOUNT_JS,
                "text/javascript; charset=utf-8",
            );
        }
        (Method::Get, "/ui/chat.js") => {
            respond_asset(req, "ui/chat.js", CHAT_JS, "text/javascript; charset=utf-8");
        }
        (Method::Get, "/ui/chat.css") => {
            respond_asset(req, "ui/chat.css", CHAT_CSS, "text/css; charset=utf-8");
        }
        (Method::Get, "/ui/stageshow.css") => {
            respond_asset(
                req,
                "ui/stageshow.css",
                STAGESHOW_CSS,
                "text/css; charset=utf-8",
            );
        }

        (Method::Get, "/ui/app.css") => {
            respond_asset(req, "ui/app.css", APP_CSS, "text/css; charset=utf-8")
        }
        (method, path)
            if path.starts_with("/craft/")
                || path.starts_with("/api/craft/")
                || matches!(path, "/ui/craft-editor.js" | "/ui/craft-merge.bundle.js") =>
        {
            craft::handle(req, method, path, query, config)
        }
        (method, path)
            if matches!(
                path,
                "/cerebro"
                    | "/cerebro/"
                    | "/ui/notion.css"
                    | "/ui/bundle.js"
                    | "/api/search"
                    | "/api/backlinks"
                    | "/api/outline"
                    | "/api/properties"
                    | "/api/graph"
                    | "/api/workspace"
            ) =>
        {
            cerebro::handle(req, method, path, query)
        }
        (method, path) => drive::handle(req, method, path, query, account.as_ref(), config),
    }
}

fn respond_asset(req: tiny_http::Request, path: &str, bundled: &str, mime: &str) {
    let _ = req.respond(
        Response::from_string(ui_asset(path, bundled))
            .with_header(Header::from_bytes("Content-Type", mime).unwrap())
            .with_header(Header::from_bytes("Cache-Control", "no-cache").unwrap())
            .with_header(Header::from_bytes("Referrer-Policy", "same-origin").unwrap()),
    );
}

fn respond_html(req: tiny_http::Request, path: &str, bundled: &str) {
    respond_asset(req, path, bundled, "text/html; charset=utf-8");
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "user-add") {
        let name = args.get(1).ok_or("Usa user-add NOMBRE")?;
        let password = if args.get(2).is_some_and(|arg| arg == "--password-stdin") {
            let mut value = String::new();
            std::io::stdin().read_line(&mut value)?;
            value.trim_end_matches(['\n', '\r']).to_owned()
        } else {
            rpassword::prompt_password("Contraseña: ")?
        };
        accounts::add_user(name, &password)?;
        println!("Cuenta creada: {name}");
        return Ok(());
    }
    let config = std::sync::Arc::new(server::Config::from_env()?);
    fs::create_dir_all(vault_root())?;
    let server = Server::http(config.address)?;
    let writes = std::sync::Arc::new(Mutex::new(()));
    drive::prepare_existing_files();
    let index_writes = std::sync::Arc::clone(&writes);
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        craft::index_ready_words(&index_writes);
    });
    println!("santolvDrive en http://{}", config.address);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for req in server.incoming_requests() {
                    handle(req, &config, &writes);
                }
            });
        }
    });
    Ok(())
}
