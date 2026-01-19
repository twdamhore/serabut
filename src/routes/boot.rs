//! Boot endpoint handler.
//!
//! GET /boot?mac={mac}
//! Returns rendered iPXE script for the MAC address.

use crate::config::AppState;
use crate::error::AppError;
use crate::services::template::TemplateContext;
use crate::services::{ActionService, HardwareService, IsoService, TemplateService};
use crate::utils::{normalize_mac, parse_host_header};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

/// Query parameters for the boot endpoint.
#[derive(Debug, Deserialize)]
pub struct BootQuery {
    pub mac: String,
}

/// Handle GET /boot?mac={mac}
///
/// Looks up the MAC in action.cfg and returns the rendered boot.ipxe.j2 template.
/// Returns 404 if MAC is not found.
pub async fn handle_boot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<BootQuery>,
) -> Result<Response, AppError> {
    let mac = normalize_mac(&query.mac)?;
    let config = state.config().await;

    tracing::info!("Boot request for MAC: {}", mac);

    // Look up MAC in action.cfg
    let action_service = ActionService::new(config.config_path.clone());
    let action = action_service.lookup(&mac)?;

    let action = match action {
        Some(a) => a,
        None => {
            tracing::info!("MAC {} not found in action.cfg, returning 404", mac);
            return Err(AppError::MacNotFound { mac });
        }
    };

    tracing::info!(
        "Found action for MAC {}: iso={}, automation={}",
        mac,
        action.iso,
        action.automation
    );

    // Load hardware config
    let hardware_service = HardwareService::new(config.config_path.clone());
    let hardware = hardware_service.load(&mac)?;

    // Get ISO config and boot template
    let iso_service = IsoService::new(config.config_path.clone());
    let iso_config = iso_service.load_config(&action.iso)?;
    let template_path = iso_service.boot_template_path(&action.iso, Some(&action.automation))?;

    // Extract host from headers
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");

    // Parse host and port from Host header
    let (parsed_host, port) = parse_host_header(host, config.port);

    tracing::info!(
        "Boot response for MAC {}: server={}:{}, file={}",
        mac, parsed_host, port, action.iso
    );

    // Build template context
    let mut ctx = TemplateContext::new(parsed_host, port, mac)
        .with_iso(action.iso)
        .with_iso_image(iso_config.filename)
        .with_automation(action.automation)
        .with_hostname(hardware.hostname)
        .with_extra(hardware.extra);

    if let Some(machine_id) = hardware.machine_id {
        ctx = ctx.with_machine_id(machine_id);
    }
    if let Some(timezone) = hardware.timezone {
        ctx = ctx.with_timezone(timezone);
    }
    if let Some(key) = hardware.base64_ssh_host_key_ecdsa_public {
        ctx = ctx.with_base64_ssh_host_key_ecdsa_public(key);
    }
    if let Some(key) = hardware.base64_ssh_host_key_ecdsa_private {
        ctx = ctx.with_base64_ssh_host_key_ecdsa_private(key);
    }
    if let Some(key) = hardware.base64_ssh_host_key_ed25519_public {
        ctx = ctx.with_base64_ssh_host_key_ed25519_public(key);
    }
    if let Some(key) = hardware.base64_ssh_host_key_ed25519_private {
        ctx = ctx.with_base64_ssh_host_key_ed25519_private(key);
    }
    if let Some(key) = hardware.base64_ssh_host_key_rsa_public {
        ctx = ctx.with_base64_ssh_host_key_rsa_public(key);
    }
    if let Some(key) = hardware.base64_ssh_host_key_rsa_private {
        ctx = ctx.with_base64_ssh_host_key_rsa_private(key);
    }

    // Render template
    let template_service = TemplateService::new();
    let rendered = template_service.render_file(&template_path, &ctx)?;

    Ok((
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        rendered,
    )
        .into_response())
}

