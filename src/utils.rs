//! Shared utility functions.

use crate::error::{AppError, AppResult};

/// Normalize MAC address to lowercase with hyphens.
///
/// Accepts MAC addresses in either colon (aa:bb:cc:dd:ee:ff) or
/// hyphen (aa-bb-cc-dd-ee-ff) format and normalizes to hyphen format.
pub fn normalize_mac(mac: &str) -> AppResult<String> {
    let mac = mac.trim().to_lowercase();
    let normalized = mac.replace(':', "-");

    if !is_valid_mac(&normalized) {
        return Err(AppError::InvalidMac { mac });
    }

    Ok(normalized)
}

/// Check if a string is a valid MAC address (aa-bb-cc-dd-ee-ff format).
pub fn is_valid_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split('-').collect();

    if parts.len() != 6 {
        return false;
    }

    parts
        .iter()
        .all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Parse host and port from Host header.
///
/// Returns the hostname and port extracted from a Host header value.
/// If no port is specified, returns the default_port.
pub fn parse_host_header(host: &str, default_port: u16) -> (String, u16) {
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
