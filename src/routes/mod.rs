//! HTTP route handlers.

pub mod action;
pub mod boot;
pub mod iso;

use crate::config::AppState;
use axum::routing::get;
use axum::Router;

/// Create the application router with all routes.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/boot", get(boot::handle_boot))
        .route("/iso/{iso_name}/{*path}", get(iso::handle_iso))
        .route("/action/remove", get(action::handle_remove))
        .with_state(state)
}
