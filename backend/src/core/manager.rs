use crate::{
    App,
    auth::User,
    error::{Error, Result},
    files, modules,
};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{FromRequestParts, Query, Request, State},
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone, Copy)]
pub struct Service {
    pub id: &'static str,
    pub schema: &'static str,
    pub page: &'static str,
    pub html: &'static str,
    pub uses_file_store: bool,
    pub worker: fn(App, &'static str) -> futures_util::future::BoxFuture<'static, ()>,
    pub routes: fn() -> Router<App>,
}

impl Service {
    pub fn app(self, core: &App) -> App {
        if self.schema == "public" {
            return core.clone();
        }
        let options = (*core.db.connect_options())
            .clone()
            .options([("search_path", format!("{},public", self.schema))]);
        let mut app = core.clone();
        app.db = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(std::time::Duration::from_secs(5))
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET statement_timeout='15s'")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("SET lock_timeout='5s'")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_lazy_with(options);
        app.journal = App::journal_pool(&app.db);
        app
    }
}

pub async fn enabled(db: &PgPool, id: &str) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT enabled FROM public.service_settings WHERE id=$1")
            .bind(id)
            .fetch_optional(db)
            .await?
            .unwrap_or(true),
    )
}

pub fn disabled() -> Error {
    Error(StatusCode::SERVICE_UNAVAILABLE, "service_disabled")
}

pub async fn lease(db: &PgPool, id: &str) -> Result<Option<Transaction<'static, Postgres>>> {
    let mut tx = db.begin().await?;
    let granted: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock_shared(hashtextextended($1,0))")
            .bind(format!("service:{id}"))
            .fetch_one(&mut *tx)
            .await?;
    if !granted {
        return Ok(None);
    }
    let on: bool = sqlx::query_scalar(
        "SELECT COALESCE((SELECT enabled FROM public.service_settings WHERE id=$1),true)",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    Ok(on.then_some(tx))
}

pub async fn set_enabled(db: &PgPool, id: &str, on: bool) -> Result<()> {
    if !modules::REGISTERED.iter().any(|service| service.id == id) {
        return Err(Error::denied());
    }
    // Serialize with workers so a completed disable cannot leave a batch running.
    let mut tx = db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("service:{id}"))
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO public.service_settings(id,enabled) VALUES($1,$2) ON CONFLICT(id) DO UPDATE SET enabled=excluded.enabled")
        .bind(id).bind(on).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn catalog(db: &PgPool) -> Result<Value> {
    let settings: Vec<(String, bool)> =
        sqlx::query_as("SELECT id,enabled FROM public.service_settings")
            .fetch_all(db)
            .await?;
    let items: Vec<_> = modules::REGISTERED
        .iter()
        .map(|module| {
            let on = settings
                .iter()
                .find(|(id, _)| id == module.id)
                .is_none_or(|(_, on)| *on);
            json!({"id":module.id,"enabled":on,"path":module.page})
        })
        .collect();
    Ok(json!({"items":items}))
}

#[derive(Clone)]
struct Registered {
    service: Service,
    app: App,
    file_routes: Router,
}

pub fn routes(core: &App) -> Router {
    let mut router = Router::new();
    let mut services = Vec::new();
    for service in modules::REGISTERED {
        let service = *service;
        let app = service.app(core);
        let db = core.db.clone();
        let registered =
            (service.routes)()
                .with_state(app.clone())
                .route_layer(middleware::from_fn(move |request: Request, next: Next| {
                    let db = db.clone();
                    async move {
                        if enabled(&db, service.id).await? {
                            Ok::<_, Error>(next.run(request).await)
                        } else {
                            Err(disabled())
                        }
                    }
                }));
        router = router.merge(registered);
        if service.uses_file_store {
            services.push(Registered {
                service,
                file_routes: crate::file_store::routes().with_state(app.clone()),
                app,
            });
        }
    }
    let state = (core.clone(), services);
    router
        .route(
            "/api/v1/services",
            axum::routing::get(|State((core, _)): State<Dispatch>| async move {
                catalog(&core.db).await.map(Json)
            }),
        )
        .route("/api/v1/activity/recent", axum::routing::get(activity))
        .fallback(dispatch)
        .with_state(state)
}

type Dispatch = (App, Vec<Registered>);

async fn dispatch(State((core, services)): State<Dispatch>, request: Request) -> Result<Response> {
    let mut path = request.uri().path().split('/').skip(1);
    if path.next() != Some("api") || path.next() != Some("v1") {
        return Err(Error::denied());
    }
    let collection = path.next();
    let id = path.next().map(str::to_owned);
    let kind = match (collection, id.as_deref(), path.next(), path.next()) {
        (
            Some("nodes"),
            Some(_),
            None | Some("touch" | "trash" | "restore" | "purge" | "content"),
            None,
        ) => "nodes",
        (Some("spaces"), Some(_), Some("nodes"), None) => "spaces",
        (Some("uploads"), Some(_), None | Some("content" | "complete"), None) => "uploads",
        (Some("uploads"), None, None, None) if request.method() == Method::POST => "spaces",
        _ => return Err(Error::denied()),
    };
    let (mut parts, body) = request.into_parts();
    User::from_request_parts(&mut parts, &core).await?;
    let (id, body) = if let Some(id) = id {
        (
            id.parse::<Uuid>().map_err(|_| Error::bad("invalid_id"))?,
            body,
        )
    } else {
        let bytes = to_bytes(body, 128 * 1024)
            .await
            .map_err(|_| Error(StatusCode::PAYLOAD_TOO_LARGE, "invalid_upload_request"))?;
        let create: crate::uploads::Create =
            serde_json::from_slice(&bytes).map_err(|_| Error::bad("invalid_upload_request"))?;
        (create.space_id, Body::from(bytes))
    };
    let lookup = match kind {
        "nodes" => "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=$1)",
        "uploads" => "SELECT EXISTS(SELECT 1 FROM uploads WHERE id=$1)",
        _ => "SELECT EXISTS(SELECT 1 FROM spaces WHERE id=$1)",
    };
    for module in services {
        if sqlx::query_scalar::<_, bool>(lookup)
            .bind(id)
            .fetch_one(&module.app.db)
            .await?
        {
            if !enabled(&core.db, module.service.id).await? {
                return Err(disabled());
            }
            return Ok(module
                .file_routes
                .oneshot(Request::from_parts(parts, body))
                .await
                .unwrap());
        }
    }
    Ok(Error::denied().into_response())
}

async fn activity(
    State((core, services)): State<Dispatch>,
    Query(query): Query<files::RecentQuery>,
    request: Request,
) -> Result<Json<Value>> {
    let (mut parts, _) = request.into_parts();
    let user = User::from_request_parts(&mut parts, &core).await?;
    let mut items = Vec::new();
    let mut boundary = None;
    for module in services {
        if !enabled(&core.db, module.service.id).await? {
            continue;
        }
        let mut page = files::recent_items(&module.app, user.id, query.clone()).await?;
        items.append(&mut page.items);
        boundary = boundary.max(page.boundary);
    }
    items.sort_unstable_by_key(|item| std::cmp::Reverse(files::recent_key(item)));
    if let Some(key) = boundary {
        items.retain(|item| files::recent_key(item) >= key);
    }
    if items.len() > 100 {
        boundary = Some(files::recent_key(&items[99]));
        items.truncate(100);
    }
    Ok(Json(
        json!({"items":items,"next_cursor":boundary.map(|(stamp,id)|format!("{stamp}:{id}"))}),
    ))
}

pub async fn console(app: &App, args: &[String]) -> Result<Value> {
    match args {
        [action] if action == "list" => catalog(&app.db).await,
        [action, id] if action == "enable" || action == "disable" => {
            let on = action == "enable";
            set_enabled(&app.db, id, on).await?;
            Ok(json!({"id":id,"enabled":on}))
        }
        _ => Err(Error::bad(
            "usage: services list | services enable ID | services disable ID",
        )),
    }
}

async fn maintenance(s: &App) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE expires_at<now()")
        .execute(&s.db)
        .await?;
    sqlx::query("DELETE FROM rate_limits WHERE window_start<now()-interval '1 day'")
        .execute(&s.db)
        .await?;
    sqlx::query("DELETE FROM login_failures WHERE updated_at<now()-interval '1 day'")
        .execute(&s.db)
        .await?;
    sqlx::query("DELETE FROM idempotency WHERE created_at<now()-interval '7 days'")
        .execute(&s.db)
        .await?;
    crate::storage::cleanup_legacy_roots(&s.root).await?;
    Ok(())
}
pub async fn run(core: App) {
    let mut workers = tokio::task::JoinSet::new();
    for service in modules::REGISTERED {
        workers.spawn((service.worker)(service.app(&core), service.id));
    }
    loop {
        if let Err(error) = maintenance(&core).await {
            tracing::error!(%error,"core maintenance failed");
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}
