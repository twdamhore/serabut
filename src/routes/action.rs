//! Action endpoint handler.
//!
//! GET /done/{mac}
//! Marks a MAC address as completed in action.cfg.

use crate::config::AppState;
use crate::error::AppError;
use crate::services::ActionService;
use crate::utils::normalize_mac;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Handle GET /done/{mac}
///
/// Comments out the MAC entry in action.cfg with a completion timestamp.
pub async fn handle_remove(
    State(state): State<AppState>,
    Path(mac): Path<String>,
) -> Result<Response, AppError> {
    let mac = normalize_mac(&mac)?;
    let config = state.config().await;

    tracing::info!("Remove request for MAC: {}", mac);

    let action_service = ActionService::new(config.config_path);
    let removed = action_service.mark_completed(&mac)?;

    if removed {
        Ok((StatusCode::OK, "OK").into_response())
    } else {
        tracing::warn!("MAC {} not found in action.cfg", mac);
        Ok((StatusCode::NOT_FOUND, "Not found").into_response())
    }
}

