use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;

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

    if !template_path.exists() {
        return Err(AppError::NotFound(format!("Template not found: {}", path)));
    }

    // Parse host header
    let host_header = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok());
    let (host, port) = parse_host_header(host_header, state.config.port);

    // Build template context
    let context = state.build_template_context(&query.hostname, &host, port)?;

    // Render template
    let rendered = template::render_template(&template_path, &context)?;

    // Determine content type
    let content_type = if path.ends_with(".ipxe.j2") {
        "text/plain"
    } else if path.ends_with(".cfg.j2") {
        "text/plain"
    } else if path.ends_with(".j2") {
        "text/plain"
    } else {
        "text/plain"
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_LENGTH, rendered.len())
        .header(header::CONTENT_TYPE, content_type)
        .body(rendered.into())
        .unwrap())
}
