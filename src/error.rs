//! Error types for the serabutd application.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::path::PathBuf;
use thiserror::Error;

/// Application error type.
#[derive(Error, Debug)]
pub enum AppError {
    #[error("MAC address not found in action.cfg: {mac}")]
    MacNotFound { mac: String },

    #[error("Hardware config not found for MAC {mac}: {path}")]
    HardwareConfigNotFound { mac: String, path: PathBuf },

    #[error("ISO config not found: {path}")]
    IsoConfigNotFound { path: PathBuf },

    #[error("ISO file not found: {path}")]
    IsoFileNotFound { path: PathBuf },

    #[error("File not found in ISO {iso}: {path}")]
    FileNotFoundInIso { iso: String, path: String },

    #[error("Template not found: {path}")]
    TemplateNotFound { path: PathBuf },

    #[error("Template rendering failed for {template}: {source}")]
    TemplateRender {
        template: String,
        #[source]
        source: minijinja::Error,
    },

    #[error("Failed to parse config file {path}: {message}")]
    ConfigParse { path: PathBuf, message: String },

    #[error("Failed to read file {path}: {source}")]
    FileRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to write file {path}: {source}")]
    FileWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to read ISO {path}: {message}")]
    IsoRead { path: PathBuf, message: String },

    #[error("Invalid MAC address format: {mac}")]
    InvalidMac { mac: String },
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::MacNotFound { .. } => StatusCode::NOT_FOUND,
            AppError::FileNotFoundInIso { .. } => StatusCode::NOT_FOUND,
            AppError::IsoFileNotFound { .. } => StatusCode::NOT_FOUND,
            AppError::HardwareConfigNotFound { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::IsoConfigNotFound { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::TemplateNotFound { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::TemplateRender { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::ConfigParse { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::FileRead { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::FileWrite { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::IsoRead { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::InvalidMac { .. } => StatusCode::BAD_REQUEST,
        };

        // Log 404s as info (expected behavior), actual errors as error
        match status {
            StatusCode::NOT_FOUND => tracing::info!("{}", self),
            StatusCode::BAD_REQUEST => tracing::warn!("{}", self),
            _ => tracing::error!("{}", self),
        }
        (status, self.to_string()).into_response()
    }
}

/// Result type alias for the application.
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_not_found_is_404() {
        let err = AppError::MacNotFound {
            mac: "aa-bb-cc-dd-ee-ff".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_hardware_config_not_found_is_500() {
        let err = AppError::HardwareConfigNotFound {
            mac: "aa-bb-cc-dd-ee-ff".to_string(),
            path: PathBuf::from("/var/lib/serabutd/hardware/aa-bb-cc-dd-ee-ff"),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_invalid_mac_is_400() {
        let err = AppError::InvalidMac {
            mac: "invalid".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
