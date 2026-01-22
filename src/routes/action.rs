use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;

use crate::config::AppState;
use crate::error::AppError;
use crate::services::template;
use crate::utils::{normalize_mac, parse_host_header};

/// GET /action/boot/{mac}
/// Return iPXE boot script for the given MAC address
pub async fn get_boot(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(mac): Path<String>,
) -> Result<Response, AppError> {
    let normalized_mac = normalize_mac(&mac);

    // Find hostname by MAC
    let hostname = state
        .hardware
        .hostname_by_mac(&normalized_mac)
        .ok_or_else(|| AppError::NotFound(format!("Unknown MAC address: {}", mac)))?;

    // Check if hostname has an action entry
    let action = state.action.read().map_err(|_| {
        AppError::Config("Failed to read action config".to_string())
    })?;

    if !action.has_entry(hostname) {
        // No action entry - machine should boot locally
        return Err(AppError::NotFound(format!(
            "No boot action for hostname: {}",
            hostname
        )));
    }

    let (release, _automation) = action
        .get(hostname)
        .ok_or_else(|| AppError::NotFound(format!("No action for hostname: {}", hostname)))?;

    drop(action); // Release lock

    // Derive OS and distro
    let os = AppState::derive_os(&release);
    let distro = AppState::derive_distro(&release);

    // Build template path: views/{os}/{distro}/{release}/boot.ipxe.j2
    let template_path = state
        .config
        .views_dir()
        .join(os)
        .join(distro)
        .join(&release)
        .join("boot.ipxe.j2");

    if !template_path.exists() {
        return Err(AppError::NotFound(format!(
            "Boot template not found: {}/{}/{}/boot.ipxe.j2",
            os, distro, release
        )));
    }

    // Parse host header
    let host_header = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let (host, port) = parse_host_header(host_header, state.config.port);

    // Build context
    let context = state.build_template_context(hostname, &host, port)?;

    // Render template
    let rendered = template::render_template(&template_path, &context)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_LENGTH, rendered.len())
        .header(header::CONTENT_TYPE, "text/plain")
        .body(rendered.into())
        .unwrap())
}

/// GET /action/done/{mac}
/// Mark installation complete by commenting out the hostname entry in action.cfg
pub async fn mark_done(
    State(state): State<Arc<AppState>>,
    Path(mac): Path<String>,
) -> Result<Response, AppError> {
    let normalized_mac = normalize_mac(&mac);

    // Find hostname by MAC
    let hostname = state
        .hardware
        .hostname_by_mac(&normalized_mac)
        .ok_or_else(|| AppError::NotFound(format!("Unknown MAC address: {}", mac)))?
        .to_string();

    // Mark done in action config
    let mut action = state.action.write().map_err(|_| {
        AppError::Config("Failed to write action config".to_string())
    })?;

    action.mark_done(&hostname)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(format!("Installation marked complete for: {}\n", hostname).into())
        .unwrap())
}
