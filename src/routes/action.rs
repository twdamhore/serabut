use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use tokio::task;

use crate::config::AppState;
use crate::error::AppError;
use crate::services::{action, template};
use crate::utils::{normalize_mac, parse_host_header};

/// GET /action/boot/{mac}
/// Return iPXE boot script for the given MAC address
pub async fn get_boot(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(mac): Path<String>,
) -> Result<Response, AppError> {
    let normalized_mac = normalize_mac(&mac);

    // Find hostname by MAC (convert to owned String to be Send across await)
    let hostname = state
        .hardware
        .hostname_by_mac(&normalized_mac)
        .ok_or_else(|| AppError::NotFound(format!("Unknown MAC address: {}", mac)))?
        .to_string();

    // Check if hostname has an action entry (use block to ensure lock is released before await)
    let release = {
        let action = state.action.read().map_err(|_| {
            AppError::Config("Failed to read action config".to_string())
        })?;

        if !action.has_entry(&hostname) {
            // No action entry - machine should boot locally
            return Err(AppError::NotFound(format!(
                "No boot action for hostname: {}",
                hostname
            )));
        }

        let (release, _automation) = action
            .get(&hostname)
            .ok_or_else(|| AppError::NotFound(format!("No action for hostname: {}", hostname)))?;

        release
    };

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
    let context = state.build_template_context(&hostname, &host, port)?;

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

    // Get the config path (short read lock)
    let config_path = {
        let action_guard = state.action.read().map_err(|_| {
            AppError::Config("Failed to read action config".to_string())
        })?;
        action_guard.path().to_path_buf()
    };

    // Do file I/O outside the lock using spawn_blocking
    let hostname_clone = hostname.clone();
    task::spawn_blocking(move || {
        action::mark_done_in_file(&config_path, &hostname_clone)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    // Update in-memory state (short write lock)
    {
        let mut action_guard = state.action.write().map_err(|_| {
            AppError::Config("Failed to write action config".to_string())
        })?;
        action_guard.remove_entry(&hostname);
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(format!("Installation marked complete for: {}\n", hostname).into())
        .unwrap())
}
