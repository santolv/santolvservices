use crate::{auth, limits, services, storage, web};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
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
    pub journal: PgPool,
    pub root: PathBuf,
    pub origin: String,
    pub frontend_origin: String,
    pub secure: bool,
    pub hashes: Arc<Semaphore>,
    pub transfers: Arc<Semaphore>,
    pub user_transfers: limits::UserTransfers,
    pub quota: i64,
    pub requests: Arc<Semaphore>,
    pub retention_days: i64,
}
impl App {
    pub fn journal_pool(db: &PgPool) -> PgPool {
        PgPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(Duration::from_secs(3))
            .after_connect(|c, _| {
                Box::pin(async move {
                    sqlx::query("SET statement_timeout='3s'")
                        .execute(&mut *c)
                        .await?;
                    sqlx::query("SET lock_timeout='2s'").execute(c).await?;
                    Ok(())
                })
            })
            .connect_lazy_with((*db.connect_options()).clone())
    }
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
            .max_connections(14)
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
        tokio::fs::create_dir_all(&root).await?;
        storage::sync_dir(&root).await?;
        if let Some(parent) = root.parent().filter(|p| !p.as_os_str().is_empty()) {
            storage::sync_dir(parent).await?;
        } else {
            storage::sync_dir(std::path::Path::new(".")).await?;
        }
        for dir in [&root, &root.join("objects"), &root.join("uploads")] {
            if tokio::fs::try_exists(dir).await? {
                crate::permissions::private_directory(dir)?;
            }
        }
        let app = Self {
            journal: Self::journal_pool(&db),
            db,
            root,
            origin,
            frontend_origin: web::default_frontend_origin(),
            secure,
            hashes: Arc::new(Semaphore::new(2)),
            transfers: Arc::new(Semaphore::new(8)),
            user_transfers: limits::UserTransfers::default(),
            quota,
            requests: Arc::new(Semaphore::new(128)),
            retention_days,
        };
        services::migrate_storage(&app).await?;
        Ok(app)
    }
}
pub fn router(s: App) -> Router {
    use axum::http::{HeaderName, HeaderValue, Method};
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin([s
            .frontend_origin
            .parse::<HeaderValue>()
            .expect("validated frontend origin")])
        .allow_credentials(tower_http::cors::AllowCredentials::predicate(
            |origin, _| origin != "null",
        ))
        .allow_methods([
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
        ])
        .allow_headers(
            [
                "content-type",
                "authorization",
                "x-csrf-token",
                "idempotency-key",
                "if-match",
                "upload-offset",
                "x-chunk-sha256",
                "range",
                "if-range",
            ]
            .map(HeaderName::from_static),
        )
        .expose_headers(
            [
                "etag",
                "content-range",
                "accept-ranges",
                "content-disposition",
            ]
            .map(HeaderName::from_static),
        )
        .max_age(Duration::from_secs(600));
    Router::new()
        .route("/", get(web::index))
        .route("/health/live", get(|| async { "ok" }))
        .route("/health/ready", get(web::health))
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/logout", post(auth::logout))
        .route("/api/v1/auth/password", post(auth::change_password))
        .route("/api/v1/me", get(auth::me))
        .route(
            "/api/v1/me/profile",
            axum::routing::put(auth::update_profile),
        )
        .route(
            "/api/v1/openapi.json",
            get(|_: auth::User| async { axum::Json(crate::openapi::document()) }),
        )
        .with_state(s.clone())
        .merge(crate::core::manager::routes(&s))
        .layer(DefaultBodyLimit::max(128 * 1024))
        .layer(middleware::from_fn_with_state(s.clone(), auth::security))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}
