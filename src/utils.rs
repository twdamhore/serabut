
/// Normalize MAC address to lowercase with hyphens (aa-bb-cc-dd-ee-ff)
pub fn normalize_mac(mac: &str) -> String {
    mac.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// Extract host and port from Host header or use defaults
pub fn parse_host_header(host_header: Option<&str>, default_port: u16) -> (String, u16) {
    match host_header {
        Some(h) => {
            if let Some((host, port_str)) = h.rsplit_once(':') {
                if let Ok(port) = port_str.parse::<u16>() {
                    return (host.to_string(), port);
                }
            }
            (h.to_string(), default_port)
        }
        None => ("localhost".to_string(), default_port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_mac() {
        assert_eq!(normalize_mac("AA:BB:CC:DD:EE:FF"), "aa-bb-cc-dd-ee-ff");
        assert_eq!(normalize_mac("aa-bb-cc-dd-ee-ff"), "aa-bb-cc-dd-ee-ff");
        assert_eq!(normalize_mac("AABBCCDDEEFF"), "aa-bb-cc-dd-ee-ff");
        assert_eq!(normalize_mac("AA-BB-CC-DD-EE-FF"), "aa-bb-cc-dd-ee-ff");
    }

    #[test]
    fn test_parse_host_header() {
        assert_eq!(parse_host_header(Some("example.com:8080"), 80), ("example.com".to_string(), 8080));
        assert_eq!(parse_host_header(Some("example.com"), 8080), ("example.com".to_string(), 8080));
        assert_eq!(parse_host_header(None, 8080), ("localhost".to_string(), 8080));
    }
}
