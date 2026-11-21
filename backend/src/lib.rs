pub mod admin;
pub mod auth;
pub mod error;
pub mod files;
pub mod installer;
pub mod openapi;
pub mod permissions;
pub mod storage;
pub mod tls;
pub mod uploads;
pub mod worker;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, patch, post},
};
use sqlx::{
    ConnectOptions, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct App {
    pub db: PgPool,
    pub root: PathBuf,
    pub origin: String,
    pub secure: bool,
    pub hashes: Arc<Semaphore>,
    pub transfers: Arc<Semaphore>,
    pub quota: i64,
    pub requests: Arc<Semaphore>,
    pub retention_days: i64,
}
impl App {
    pub async fn connect(
        url: &str,
        root: PathBuf,
        origin: String,
        secure: bool,
        quota: i64,
        retention_days: i64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if !origin.starts_with("https://") && secure {
            return Err("production APP_ORIGIN must use HTTPS".into());
        }
        if origin.ends_with('/') || quota <= 0 {
            return Err("invalid origin or quota".into());
        }
        if !(1..=36500).contains(&retention_days) {
            return Err("TRASH_RETENTION_DAYS must be 1..36500".into());
        }
        let options: PgConnectOptions = url.parse()?;
        let local = matches!(options.get_host(), "localhost" | "127.0.0.1" | "::1");
        if !local && !matches!(options.get_ssl_mode(), PgSslMode::VerifyFull) {
            return Err("remote PostgreSQL requires sslmode=verify-full".into());
        }
        let options = options.disable_statement_logging();
        let db = PgPoolOptions::new()
            .max_connections(16)
            .acquire_timeout(Duration::from_secs(5))
            .after_connect(|c, _| {
                Box::pin(async move {
                    sqlx::query("SET statement_timeout='15s'")
                        .execute(&mut *c)
                        .await?;
                    sqlx::query("SET lock_timeout='5s'")
                        .execute(&mut *c)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&db).await?;
        tokio::fs::create_dir_all(root.join("objects")).await?;
        tokio::fs::create_dir_all(root.join("uploads")).await?;
        storage::sync_dir(&root).await?;
        if let Some(parent) = root.parent().filter(|p| !p.as_os_str().is_empty()) {
            storage::sync_dir(parent).await?;
        } else {
            storage::sync_dir(std::path::Path::new(".")).await?;
        }
        for dir in [&root, &root.join("objects"), &root.join("uploads")] {
            crate::permissions::private_directory(dir)?;
        }
        Ok(Self {
            db,
            root,
            origin,
            secure,
            hashes: Arc::new(Semaphore::new(2)),
            transfers: Arc::new(Semaphore::new(8)),
            quota,
            requests: Arc::new(Semaphore::new(128)),
            retention_days,
        })
    }
}
pub fn router(s: App) -> Router {
    Router::new()
        .route("/health/live", get(|| async { "ok" }))
        .route("/health/ready", get(worker::health))
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/logout", post(auth::logout))
        .route("/api/v1/auth/password", post(auth::change_password))
        .route("/api/v1/admin/users", get(admin::list).post(admin::create))
        .route("/api/v1/admin/users/{id}", patch(admin::set_disabled))
        .route(
            "/api/v1/admin/users/{id}/password",
            post(admin::reset_password),
        )
        .route("/api/v1/admin/audit", get(admin::audit_log))
        .route("/api/v1/me", get(auth::me))
        .route("/api/v1/shared", get(files::shared))
        .route(
            "/api/v1/openapi.json",
            get(|| async { axum::Json(crate::openapi::document()) }),
        )
        .route(
            "/api/v1/spaces",
            get(files::spaces).post(files::create_space),
        )
        .route(
            "/api/v1/spaces/{space}/members",
            get(files::members).put(files::set_member),
        )
        .route(
            "/api/v1/spaces/{space}/members/{user}",
            axum::routing::delete(files::remove_member),
        )
        .route(
            "/api/v1/spaces/{space}/nodes",
            get(files::list).post(files::create_folder),
        )
        .route(
            "/api/v1/nodes/{id}",
            get(files::get_node).patch(files::edit_node),
        )
        .route("/api/v1/nodes/{id}/trash", post(files::trash))
        .route("/api/v1/nodes/{id}/restore", post(files::restore))
        .route("/api/v1/nodes/{id}/purge", post(files::purge))
        .route(
            "/api/v1/nodes/{id}/grants",
            get(files::grants).put(files::set_grant),
        )
        .route(
            "/api/v1/nodes/{id}/grants/revoke",
            post(files::revoke_grant),
        )
        .route("/api/v1/nodes/{id}/versions", get(files::versions))
        .route(
            "/api/v1/nodes/{id}/versions/{version}/restore",
            post(files::restore_version),
        )
        .route("/api/v1/nodes/{id}/content", get(uploads::download))
        .route("/api/v1/uploads", post(uploads::create))
        .route(
            "/api/v1/uploads/{id}",
            get(uploads::status).delete(uploads::cancel),
        )
        .route("/api/v1/uploads/{id}/content", patch(uploads::append))
        .route("/api/v1/uploads/{id}/complete", post(uploads::complete))
        .layer(DefaultBodyLimit::max(128 * 1024))
        .layer(middleware::from_fn_with_state(s.clone(), auth::security))
        .layer(TraceLayer::new_for_http())
        .with_state(s)
}
