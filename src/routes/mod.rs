//! HTTP route handlers.

pub mod action;
pub mod boot;
pub mod iso;

use crate::config::AppState;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;

/// HTTP request logging middleware.
///
/// Logs each request in format: "IP METHOD PATH - STATUS"
async fn request_logging(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();

    let response = next.run(request).await;

    let status = response.status();
    tracing::info!("{} {} {} - {}", addr.ip(), method, uri, status.as_u16());

    response
}

/// Create the application router with all routes.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/boot", get(boot::handle_boot))
        .route("/iso/{iso_name}/{*path}", get(iso::handle_iso))
        .route("/action/remove", get(action::handle_remove))
        .layer(middleware::from_fn(request_logging))
        .with_state(state)
}
