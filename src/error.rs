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

#[cfg(test)]
mod tests {
    use super::*;

    mod capture_error_tests {
        use super::*;

        #[test]
        fn test_interface_not_found_display() {
            let err = CaptureError::InterfaceNotFound("eth0".to_string());
            assert_eq!(err.to_string(), "failed to find network interface: eth0");
        }

        #[test]
        fn test_channel_creation_display() {
            let err = CaptureError::ChannelCreation("socket error".to_string());
            assert_eq!(err.to_string(), "failed to create capture channel: socket error");
        }

        #[test]
        fn test_insufficient_permissions_display() {
            let err = CaptureError::InsufficientPermissions;
            assert_eq!(
                err.to_string(),
                "insufficient permissions to capture packets - try running with sudo"
            );
        }

        #[test]
        fn test_capture_display() {
            let err = CaptureError::Capture("read failed".to_string());
            assert_eq!(err.to_string(), "capture error: read failed");
        }
    }

    mod parse_error_tests {
        use super::*;

        #[test]
        fn test_packet_too_short_display() {
            let err = ParseError::PacketTooShort {
                expected: 240,
                actual: 100,
            };
            assert_eq!(
                err.to_string(),
                "packet too short: expected at least 240 bytes, got 100"
            );
        }

        #[test]
        fn test_invalid_magic_cookie_display() {
            let err = ParseError::InvalidMagicCookie;
            assert_eq!(err.to_string(), "invalid DHCP magic cookie");
        }

        #[test]
        fn test_invalid_option_display() {
            let err = ParseError::InvalidOption {
                offset: 240,
                message: "truncated".to_string(),
            };
            assert_eq!(
                err.to_string(),
                "invalid option at offset 240: truncated"
            );
        }

        #[test]
        fn test_not_dhcp_display() {
            let err = ParseError::NotDhcp;
            assert_eq!(err.to_string(), "not a DHCP packet");
        }

        #[test]
        fn test_invalid_utf8_from() {
            let invalid_bytes = vec![0xff, 0xfe];
            let utf8_err = String::from_utf8(invalid_bytes).unwrap_err();
            let err: ParseError = utf8_err.into();
            assert!(err.to_string().contains("invalid UTF-8"));
        }
    }

    mod app_error_tests {
        use super::*;

        #[test]
        fn test_from_capture_error() {
            let capture_err = CaptureError::InsufficientPermissions;
            let app_err: AppError = capture_err.into();
            assert!(app_err.to_string().contains("capture error"));
        }

        #[test]
        fn test_from_parse_error() {
            let parse_err = ParseError::InvalidMagicCookie;
            let app_err: AppError = parse_err.into();
            assert!(app_err.to_string().contains("parse error"));
        }

        #[test]
        fn test_config_display() {
            let err = AppError::Config("invalid interface".to_string());
            assert_eq!(err.to_string(), "configuration error: invalid interface");
        }
    }
}
