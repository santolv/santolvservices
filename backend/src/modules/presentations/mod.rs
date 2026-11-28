//! The deckcraft editor (ArtCraft), hosted in the suite. Its files are Drive files: the page opens
//! them through the Drive API and saves new versions with Drive's revision checks, so this module
//! adds no routes of its own.
use crate::{App, core::manager::Service};
use axum::Router;

pub const SERVICE: Service = Service {
    id: "presentations",
    schema: "public",
    page: "/presentations",
    html: include_str!("../../../../frontend/presentations/index.html"),
    uses_file_store: false,
    requires: Some("drive"),
    worker: |_, _| Box::pin(async {}),
    routes,
};

fn routes() -> Router<App> {
    Router::new()
}
