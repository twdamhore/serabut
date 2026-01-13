//! Error types for the DHCP PXE listener.
//!
//! Using thiserror for ergonomic error definitions.

use thiserror::Error;

/// Errors that can occur during packet capture.
#[derive(Error, Debug)]
pub enum CaptureError {
    #[error("failed to find network interface: {0}")]
    InterfaceNotFound(String),

    #[error("failed to create capture channel: {0}")]
    ChannelCreation(String),

    #[error("insufficient permissions to capture packets - try running with sudo")]
    InsufficientPermissions,

    #[error("capture error: {0}")]
    Capture(String),
}

/// Errors that can occur during DHCP packet parsing.
#[derive(Error, Debug)]
pub enum ParseError {
    #[error("packet too short: expected at least {expected} bytes, got {actual}")]
    PacketTooShort { expected: usize, actual: usize },

    #[error("invalid DHCP magic cookie")]
    InvalidMagicCookie,

    #[error("invalid option at offset {offset}: {message}")]
    InvalidOption { offset: usize, message: String },

    #[error("not a DHCP packet")]
    NotDhcp,

    #[error("invalid UTF-8 in string field: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
}

/// Top-level application errors.
#[derive(Error, Debug)]
pub enum AppError {
    #[error("capture error: {0}")]
    Capture(#[from] CaptureError),

    #[error("parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("configuration error: {0}")]
    Config(String),
}
