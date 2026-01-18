//! Action endpoint handler.
//!
//! GET /action/remove?mac={mac}
//! Marks a MAC address as completed in action.cfg.

use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::services::ActionService;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

/// Query parameters for the action/remove endpoint.
#[derive(Debug, Deserialize)]
pub struct RemoveQuery {
    pub mac: String,
}

/// Handle GET /action/remove?mac={mac}
///
/// Comments out the MAC entry in action.cfg with a completion timestamp.
pub async fn handle_remove(
    State(state): State<AppState>,
    Query(query): Query<RemoveQuery>,
) -> Result<Response, AppError> {
    let mac = normalize_mac(&query.mac)?;
    let config = state.config().await;

    tracing::info!("Remove request for MAC: {}", mac);

    let action_service = ActionService::new(config.config_path);
    let removed = action_service.mark_completed(&mac)?;

    if removed {
        tracing::info!("Marked MAC {} as completed", mac);
        Ok((StatusCode::OK, "OK").into_response())
    } else {
        tracing::warn!("MAC {} not found in action.cfg", mac);
        Ok((StatusCode::NOT_FOUND, "Not found").into_response())
    }
}

/// Normalize MAC address to lowercase with hyphens.
fn normalize_mac(mac: &str) -> AppResult<String> {
    let mac = mac.trim().to_lowercase();
    let normalized = mac.replace(':', "-");

    if !is_valid_mac(&normalized) {
        return Err(AppError::InvalidMac { mac });
    }

    Ok(normalized)
}

/// Check if a string is a valid MAC address.
fn is_valid_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split('-').collect();

    if parts.len() != 6 {
        return false;
    }

    parts
        .iter()
        .all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_mac() {
        assert_eq!(
            normalize_mac("AA-BB-CC-DD-EE-FF").unwrap(),
            "aa-bb-cc-dd-ee-ff"
        );
        assert_eq!(
            normalize_mac("aa:bb:cc:dd:ee:ff").unwrap(),
            "aa-bb-cc-dd-ee-ff"
        );
    }

    #[test]
    fn test_normalize_mac_invalid() {
        assert!(normalize_mac("invalid").is_err());
        assert!(normalize_mac("aa-bb-cc").is_err());
    }

    #[test]
    fn test_is_valid_mac() {
        assert!(is_valid_mac("aa-bb-cc-dd-ee-ff"));
        assert!(!is_valid_mac("invalid"));
        assert!(!is_valid_mac("aa-bb-cc-dd-ee"));
    }
}
