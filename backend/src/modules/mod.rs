pub mod drive;
pub mod photos;

use crate::{App, core::manager::Service, file_store};
use axum::Router;

pub const REGISTERED: &[Service] = &[drive::SERVICE, photos::SERVICE];

pub fn common_files() -> Router<App> {
    use axum::routing::{get, patch, post};
    use file_store::{files, uploads};
    Router::new()
        .route(
            "/api/v1/spaces/{space}/nodes",
            get(files::list).post(files::create_folder),
        )
        .route(
            "/api/v1/nodes/{id}",
            get(files::get_node).patch(files::edit_node),
        )
        .route("/api/v1/nodes/{id}/touch", post(files::touch))
        .route("/api/v1/nodes/{id}/trash", post(files::trash))
        .route("/api/v1/nodes/{id}/restore", post(files::restore))
        .route("/api/v1/nodes/{id}/purge", post(files::purge))
        .route("/api/v1/nodes/{id}/content", get(uploads::download))
        .route("/api/v1/uploads", post(uploads::create))
        .route(
            "/api/v1/uploads/{id}",
            get(uploads::status).delete(uploads::cancel),
        )
        .route("/api/v1/uploads/{id}/content", patch(uploads::append))
        .route("/api/v1/uploads/{id}/complete", post(uploads::complete))
        .layer(axum::extract::DefaultBodyLimit::max(128 * 1024))
}
