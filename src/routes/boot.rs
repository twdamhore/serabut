//! Boot endpoint handler.
//!
//! GET /boot?mac={mac}
//! Returns rendered iPXE script for the MAC address.

use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::services::{ActionService, HardwareService, IsoService, TemplateService};
use crate::services::template::TemplateContext;
use axum::extract::{Host, Query, State};
use axum::http::StatusCode;
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
    Host(host): Host,
    Query(query): Query<BootQuery>,
) -> Result<Response, AppError> {
    let mac = normalize_mac(&query.mac)?;
    let config = state.config().await;

    tracing::debug!("Boot request for MAC: {}", mac);

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

    // Get boot template
    let iso_service = IsoService::new(config.config_path.clone());
    let template_path = iso_service.boot_template_path(&action.iso)?;

    // Parse host and port from Host header
    let (parsed_host, port) = parse_host_header(&host, config.port);

    // Build template context
    let ctx = TemplateContext::new(parsed_host, port, mac)
        .with_iso(action.iso)
        .with_automation(action.automation)
        .with_hostname(hardware.hostname)
        .with_extra(hardware.extra);

    // Render template
    let template_service = TemplateService::new();
    let rendered = template_service.render_file(&template_path, &ctx)?;

    tracing::debug!("Rendered boot template successfully");

    Ok((
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        rendered,
    )
        .into_response())
}

/// Normalize MAC address to lowercase with hyphens.
fn normalize_mac(mac: &str) -> AppResult<String> {
    let mac = mac.trim().to_lowercase();

    // Validate MAC format (aa-bb-cc-dd-ee-ff or aa:bb:cc:dd:ee:ff)
    let normalized = mac.replace(':', "-");

    if !is_valid_mac(&normalized) {
        return Err(AppError::InvalidMac { mac });
    }

    Ok(normalized)
}

/// Check if a string is a valid MAC address (aa-bb-cc-dd-ee-ff format).
fn is_valid_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split('-').collect();

    if parts.len() != 6 {
        return false;
    }

    parts.iter().all(|part| {
        part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// Parse host and port from Host header.
fn parse_host_header(host: &str, default_port: u16) -> (String, u16) {
    if let Some((h, p)) = host.rsplit_once(':') {
        if let Ok(port) = p.parse::<u16>() {
            return (h.to_string(), port);
        }
    }
    (host.to_string(), default_port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_mac_hyphen() {
        let result = normalize_mac("AA-BB-CC-DD-EE-FF").unwrap();
        assert_eq!(result, "aa-bb-cc-dd-ee-ff");
    }

    #[test]
    fn test_normalize_mac_colon() {
        let result = normalize_mac("aa:bb:cc:dd:ee:ff").unwrap();
        assert_eq!(result, "aa-bb-cc-dd-ee-ff");
    }

    #[test]
    fn test_normalize_mac_invalid() {
        let result = normalize_mac("invalid");
        assert!(matches!(result, Err(AppError::InvalidMac { .. })));
    }

    #[test]
    fn test_normalize_mac_too_short() {
        let result = normalize_mac("aa-bb-cc");
        assert!(matches!(result, Err(AppError::InvalidMac { .. })));
    }

    #[test]
    fn test_is_valid_mac() {
        assert!(is_valid_mac("aa-bb-cc-dd-ee-ff"));
        assert!(is_valid_mac("00-11-22-33-44-55"));
        assert!(!is_valid_mac("invalid"));
        assert!(!is_valid_mac("aa-bb-cc"));
        assert!(!is_valid_mac("aa-bb-cc-dd-ee-gg")); // invalid hex
    }

    #[test]
    fn test_parse_host_header_with_port() {
        let (host, port) = parse_host_header("192.168.1.1:8080", 4123);
        assert_eq!(host, "192.168.1.1");
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_parse_host_header_without_port() {
        let (host, port) = parse_host_header("192.168.1.1", 4123);
        assert_eq!(host, "192.168.1.1");
        assert_eq!(port, 4123);
    }

    #[test]
    fn test_parse_host_header_hostname() {
        let (host, port) = parse_host_header("pxe.local:4123", 4123);
        assert_eq!(host, "pxe.local");
        assert_eq!(port, 4123);
    }
}
