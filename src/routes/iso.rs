//! ISO endpoint handler.
//!
//! GET /iso/{iso_name}/{path}
//! Serves ISO files, templates, or files from within ISOs.

use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::services::template::TemplateContext;
use crate::services::{HardwareService, IsoService, TemplateService};
use crate::utils::{normalize_mac, parse_host_header};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;
use tokio::fs::File;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;

/// Query parameters for the ISO endpoint.
#[derive(Debug, Deserialize, Default)]
pub struct IsoQuery {
    pub mac: Option<String>,
}

/// Handle GET /iso/{iso_name}/{path}
///
/// Four behaviors:
/// 1. If path matches initrd_path and firmware is configured -> serve combined initrd+firmware
/// 2. If path matches the ISO filename -> serve the whole ISO
/// 3. If path.j2 exists in config dir -> render template
/// 4. Otherwise -> read from ISO via iso9660_simple
pub async fn handle_iso(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((iso_name, path)): Path<(String, String)>,
    Query(query): Query<IsoQuery>,
) -> Result<Response, AppError> {
    let config = state.config().await;
    let iso_service = IsoService::new(config.config_path.clone());

    tracing::debug!("ISO request: iso={}, path={}", iso_name, path);

    // Check if this is a request for initrd that needs firmware concatenation
    if let Some((initrd_path, firmware)) = iso_service.should_concat_firmware(&iso_name, &path)? {
        tracing::info!(
            "Serving initrd with firmware: {}/{} + {}",
            iso_name,
            initrd_path,
            firmware
        );
        return serve_initrd_with_firmware(&iso_service, &iso_name, &initrd_path, &firmware);
    }

    // Check if this is a request for the ISO file itself
    if iso_service.is_iso_file(&iso_name, &path)? {
        tracing::info!("Serving ISO file: {}/{}", iso_name, path);
        return serve_iso_file(&iso_service, &iso_name).await;
    }

    // Extract host from headers
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");

    // Check if a template exists for this path
    if let Some(template_path) = iso_service.template_path(&iso_name, &path) {
        tracing::info!("Rendering template: {}/{}", iso_name, path);
        return serve_template(
            &config.config_path,
            &template_path,
            host,
            config.port,
            &iso_name,
            &path,
            query.mac.as_deref(),
        )
        .await;
    }

    // Otherwise, read from ISO via iso9660_simple
    tracing::info!("Reading from ISO: {}/{}", iso_name, path);
    serve_from_iso(&iso_service, &iso_name, &path)
}

/// Serve the ISO file itself for streaming.
async fn serve_iso_file(iso_service: &IsoService, iso_name: &str) -> AppResult<Response> {
    let iso_path = iso_service.iso_file_path(iso_name)?;

    let file = File::open(&iso_path).await.map_err(|e| AppError::FileRead {
        path: iso_path.clone(),
        source: e,
    })?;

    let metadata = file.metadata().await.map_err(|e| AppError::FileRead {
        path: iso_path.clone(),
        source: e,
    })?;
    let content_length = metadata.len();

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, content_length)
        .body(body)
        .unwrap())
}

/// Serve a rendered template.
async fn serve_template(
    config_path: &std::path::Path,
    template_path: &std::path::Path,
    host: &str,
    default_port: u16,
    iso_name: &str,
    path: &str,
    mac: Option<&str>,
) -> AppResult<Response> {
    // Parse host and port
    let (parsed_host, port) = parse_host_header(host, default_port);

    // Extract MAC and automation from path if present
    // Path format: automation/{automation}/{mac}/{file}
    let (automation, mac) = extract_automation_and_mac(path, mac)?;

    // Build template context
    let mut ctx = TemplateContext::new(parsed_host, port, mac.clone())
        .with_iso(iso_name.to_string());

    if let Some(auto) = automation {
        ctx = ctx.with_automation(auto);
    }

    // Load hardware config if we have a MAC
    let hardware_service = HardwareService::new(config_path.to_path_buf());
    let hardware = hardware_service.load(&mac)?;
    ctx = ctx.with_hostname(hardware.hostname).with_extra(hardware.extra);

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
    let rendered = template_service.render_file(template_path, &ctx)?;
    let content_length = rendered.len();

    // Determine content type based on file extension
    let content_type = guess_content_type(path);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, content_length)
        .body(Body::from(rendered))
        .unwrap())
}

/// Serve a file from within the ISO using streaming.
fn serve_from_iso(iso_service: &IsoService, iso_name: &str, path: &str) -> AppResult<Response> {
    let (content_length, receiver) = iso_service.stream_from_iso(iso_name, path)?;
    let stream = ReceiverStream::new(receiver);
    let body = Body::from_stream(stream);

    // Determine content type based on file extension
    let content_type = guess_content_type(path);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, content_length)
        .body(body)
        .unwrap())
}

/// Serve initrd with firmware concatenated using streaming.
///
/// Used for Debian netboot where firmware.cpio.gz needs to be appended to initrd.
fn serve_initrd_with_firmware(
    iso_service: &IsoService,
    iso_name: &str,
    initrd_path: &str,
    firmware: &str,
) -> AppResult<Response> {
    let (content_length, receiver) =
        iso_service.stream_initrd_with_firmware(iso_name, initrd_path, firmware)?;
    let stream = ReceiverStream::new(receiver);
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, content_length)
        .body(body)
        .unwrap())
}

/// Extract automation name and MAC from path.
///
/// Path format: automation/{automation}/{mac}/{file}
/// Returns (automation, mac)
fn extract_automation_and_mac(path: &str, query_mac: Option<&str>) -> AppResult<(Option<String>, String)> {
    let parts: Vec<&str> = path.split('/').collect();

    // Check if path matches automation/{automation}/{mac}/{file}
    if parts.len() >= 4 && parts[0] == "automation" {
        let automation = parts[1].to_string();
        let mac = normalize_mac(parts[2])?;
        return Ok((Some(automation), mac));
    }

    // Fall back to query parameter
    if let Some(mac) = query_mac {
        let mac = normalize_mac(mac)?;
        return Ok((None, mac));
    }

    // No MAC available - this shouldn't happen for templates that need it
    Err(AppError::InvalidMac {
        mac: "missing".to_string(),
    })
}

/// Guess content type from file extension.
fn guess_content_type(path: &str) -> &'static str {
    if path.ends_with(".iso") {
        "application/octet-stream"
    } else if path.ends_with(".j2")
        || path.ends_with(".yaml")
        || path.ends_with(".yml")
        || path.ends_with(".ks")
    {
        "text/plain; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_automation_and_mac_from_path() {
        let (auto, mac) =
            extract_automation_and_mac("automation/docker/aa-bb-cc-dd-ee-ff/user-data", None)
                .unwrap();
        assert_eq!(auto, Some("docker".to_string()));
        assert_eq!(mac, "aa-bb-cc-dd-ee-ff");
    }

    #[test]
    fn test_extract_automation_and_mac_from_query() {
        let (auto, mac) =
            extract_automation_and_mac("some/path", Some("aa-bb-cc-dd-ee-ff")).unwrap();
        assert_eq!(auto, None);
        assert_eq!(mac, "aa-bb-cc-dd-ee-ff");
    }

    #[test]
    fn test_extract_automation_and_mac_missing() {
        let result = extract_automation_and_mac("some/path", None);
        assert!(matches!(result, Err(AppError::InvalidMac { .. })));
    }

    #[test]
    fn test_guess_content_type() {
        assert_eq!(guess_content_type("file.iso"), "application/octet-stream");
        assert_eq!(guess_content_type("user-data"), "application/octet-stream");
        assert_eq!(guess_content_type("config.yaml"), "text/plain; charset=utf-8");
        assert_eq!(guess_content_type("kickstart.ks"), "text/plain; charset=utf-8");
        assert_eq!(guess_content_type("data.json"), "application/json");
    }

    #[test]
    fn test_parse_host_header() {
        let (host, port) = parse_host_header("192.168.1.1:8080", 4123);
        assert_eq!(host, "192.168.1.1");
        assert_eq!(port, 8080);

        let (host, port) = parse_host_header("pxe.local", 4123);
        assert_eq!(host, "pxe.local");
        assert_eq!(port, 4123);
    }
}
