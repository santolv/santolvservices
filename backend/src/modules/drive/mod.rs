mod domain;
use crate::{App, core::manager::Service};
use axum::{
    Router,
    routing::{delete, get, post},
};

pub const SERVICE: Service = Service {
    id: "drive",
    schema: "public",
    page: "/drive",
    html: include_str!("../../../../frontend/drive/index.html"),
    uses_file_store: true,
    worker: |app, id| Box::pin(crate::worker::run(app, id)),
    routes,
};

fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/shared", get(domain::shared))
        .route("/api/v1/recent", get(domain::recent))
        .route(
            "/api/v1/spaces",
            get(domain::spaces).post(domain::create_space),
        )
        .route("/api/v1/spaces/{space}", delete(domain::delete_space))
        .route(
            "/api/v1/spaces/{space}/members",
            get(domain::members).put(domain::set_member),
        )
        .route(
            "/api/v1/spaces/{space}/members/{user}",
            delete(domain::remove_member),
        )
        .route(
            "/api/v1/nodes/{id}/grants",
            get(domain::grants).put(domain::set_grant),
        )
        .route(
            "/api/v1/nodes/{id}/grants/revoke",
            post(domain::revoke_grant),
        )
}
