//! The pdfcraft editor (ArtCraft), hosted in the suite. Its files are Drive files: the page opens
//! them through the Drive API and saves new versions with Drive's revision checks, so this module
//! adds no routes of its own.
use crate::{App, core::manager::Service};
use axum::Router;

pub const SERVICE: Service = Service {
    id: "pdf",
    schema: "public",
    page: "/pdf",
    html: include_str!("../../../../frontend/pdf/index.html"),
    uses_file_store: false,
    requires: Some("drive"),
    worker: |_, _| Box::pin(async {}),
    routes,
};

fn routes() -> Router<App> {
    Router::new()
}
