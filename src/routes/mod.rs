use std::sync::Arc;

use axum::routing::get;
use axum::Router;

use crate::config::AppState;

mod action;
mod content;
mod views;

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Content routes
        .route("/content/iso/{release}/{*path}", get(content::get_iso_content))
        .route("/content/combine/{name}", get(content::get_combined_content))
        .route("/content/raw/{release}/{filename}", get(content::get_raw_content))
        // Views route
        .route("/views/{*path}", get(views::get_view))
        // Action routes
        .route("/action/boot/{mac}", get(action::get_boot))
        .route("/action/done/{mac}", get(action::mark_done))
        .with_state(state)
}
