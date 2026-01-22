use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use tokio::task;

use crate::config::AppState;
use crate::error::AppError;
use crate::services::template;
use crate::utils::parse_host_header;

#[derive(Deserialize)]
pub struct ViewsQuery {
    hostname: String,
}

/// GET /views/{*path}?hostname={hostname}
/// Render Jinja2 template with context
pub async fn get_view(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(path): Path<String>,
    Query(query): Query<ViewsQuery>,
) -> Result<Response, AppError> {
    let template_path = state.config.views_dir().join(&path);

    // Check existence using spawn_blocking for async safety
    let path_for_check = template_path.clone();
    let exists = task::spawn_blocking(move || path_for_check.exists())
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if !exists {
        return Err(AppError::NotFound(format!("Template not found: {}", path)));
    }

    // Parse host header
    let host_header = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok());
    let (host, port) = parse_host_header(host_header, state.config.port);

    // Build template context
    let context = state.build_template_context(&query.hostname, &host, port)?;

    // Render template using spawn_blocking for sync file I/O
    let rendered = task::spawn_blocking(move || {
        template::render_template(template_path, context)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_LENGTH, rendered.len())
        .header(header::CONTENT_TYPE, "text/plain")
        .body(rendered.into())
        .unwrap())
}
