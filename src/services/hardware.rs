//! Hardware service for managing hardware.cfg files.
//!
//! Each hardware directory contains configuration for a specific MAC address.

use crate::error::{AppError, AppResult};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

/// Hardware configuration for a MAC address.
#[derive(Debug, Clone)]
pub struct HardwareConfig {
    pub hostname: String,
    /// Optional machine identifier.
    pub machine_id: Option<String>,
    /// Base64-encoded SSH host keys.
    pub base64_ssh_host_key_ecdsa_public: Option<String>,
    pub base64_ssh_host_key_ecdsa_private: Option<String>,
    pub base64_ssh_host_key_ed25519_public: Option<String>,
    pub base64_ssh_host_key_ed25519_private: Option<String>,
    pub base64_ssh_host_key_rsa_public: Option<String>,
    pub base64_ssh_host_key_rsa_private: Option<String>,
    /// Additional key-value pairs from hardware.cfg.
    pub extra: HashMap<String, String>,
}

/// Service for reading hardware configurations.
pub struct HardwareService {
    config_path: PathBuf,
}

impl HardwareService {
    /// Create a new hardware service.
    pub fn new(config_path: PathBuf) -> Self {
        Self { config_path }
    }

    /// Get the path to a hardware directory for a MAC.
    fn hardware_dir(&self, mac: &str) -> PathBuf {
        self.config_path.join("hardware").join(mac)
    }

    /// Get the path to hardware.cfg for a MAC.
    fn hardware_cfg_path(&self, mac: &str) -> PathBuf {
        self.hardware_dir(mac).join("hardware.cfg")
    }

    /// Load hardware configuration for a MAC address.
    ///
    /// Returns an error if the hardware.cfg doesn't exist.
    pub fn load(&self, mac: &str) -> AppResult<HardwareConfig> {
        let path = self.hardware_cfg_path(mac);

        if !path.exists() {
            return Err(AppError::HardwareConfigNotFound {
                mac: mac.to_string(),
                path,
            });
        }

        let file = File::open(&path).map_err(|e| AppError::FileRead {
            path: path.clone(),
            source: e,
        })?;

        let reader = BufReader::new(file);
        let mut hostname = None;
        let mut machine_id = None;
        let mut base64_ssh_host_key_ecdsa_public = None;
        let mut base64_ssh_host_key_ecdsa_private = None;
        let mut base64_ssh_host_key_ed25519_public = None;
        let mut base64_ssh_host_key_ed25519_private = None;
        let mut base64_ssh_host_key_rsa_public = None;
        let mut base64_ssh_host_key_rsa_private = None;
        let mut extra = HashMap::new();

        for line in reader.lines() {
            let line = line.map_err(|e| AppError::FileRead {
                path: path.clone(),
                source: e,
            })?;

            if let Some((key, value)) = parse_config_line(&line) {
                match key {
                    "hostname" => hostname = Some(value.to_string()),
                    "machine_id" => machine_id = Some(value.to_string()),
                    "base64_ssh_host_key_ecdsa_public" => base64_ssh_host_key_ecdsa_public = Some(value.to_string()),
                    "base64_ssh_host_key_ecdsa_private" => base64_ssh_host_key_ecdsa_private = Some(value.to_string()),
                    "base64_ssh_host_key_ed25519_public" => base64_ssh_host_key_ed25519_public = Some(value.to_string()),
                    "base64_ssh_host_key_ed25519_private" => base64_ssh_host_key_ed25519_private = Some(value.to_string()),
                    "base64_ssh_host_key_rsa_public" => base64_ssh_host_key_rsa_public = Some(value.to_string()),
                    "base64_ssh_host_key_rsa_private" => base64_ssh_host_key_rsa_private = Some(value.to_string()),
                    _ => { extra.insert(key.to_string(), value.to_string()); }
                }
            }
        }

        let hostname = hostname.ok_or_else(|| AppError::ConfigParse {
            path: path.clone(),
            message: "Missing required 'hostname' field".to_string(),
        })?;

        Ok(HardwareConfig {
            hostname,
            machine_id,
            base64_ssh_host_key_ecdsa_public,
            base64_ssh_host_key_ecdsa_private,
            base64_ssh_host_key_ed25519_public,
            base64_ssh_host_key_ed25519_private,
            base64_ssh_host_key_rsa_public,
            base64_ssh_host_key_rsa_private,
            extra,
        })
    }
}

/// Parse a key=value line, skipping comments and empty lines.
fn parse_config_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();

    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let (key, value) = line.split_once('=')?;
    Some((key.trim(), value.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_parse_config_line() {
        assert_eq!(parse_config_line("hostname=server01"), Some(("hostname", "server01")));
        assert_eq!(parse_config_line("  key = value  "), Some(("key", "value")));
        assert_eq!(parse_config_line("# comment"), None);
        assert_eq!(parse_config_line(""), None);
        assert_eq!(parse_config_line("no-equals"), None);
    }

    #[test]
    fn test_load_hardware_config() {
        let dir = setup_test_dir();
        let mac = "aa-bb-cc-dd-ee-ff";
        let hardware_dir = dir.path().join("hardware").join(mac);
        std::fs::create_dir_all(&hardware_dir).unwrap();
        std::fs::write(
            hardware_dir.join("hardware.cfg"),
            "hostname=server01\nrole=webserver\n",
        )
        .unwrap();

        let service = HardwareService::new(dir.path().to_path_buf());
        let config = service.load(mac).unwrap();

        assert_eq!(config.hostname, "server01");
        assert_eq!(config.machine_id, None);
        assert_eq!(config.extra.get("role"), Some(&"webserver".to_string()));
    }

    #[test]
    fn test_load_hardware_config_with_machine_id() {
        let dir = setup_test_dir();
        let mac = "aa-bb-cc-dd-ee-ff";
        let hardware_dir = dir.path().join("hardware").join(mac);
        std::fs::create_dir_all(&hardware_dir).unwrap();
        std::fs::write(
            hardware_dir.join("hardware.cfg"),
            "hostname=server01\nmachine_id=srv-001\nrole=webserver\n",
        )
        .unwrap();

        let service = HardwareService::new(dir.path().to_path_buf());
        let config = service.load(mac).unwrap();

        assert_eq!(config.hostname, "server01");
        assert_eq!(config.machine_id, Some("srv-001".to_string()));
        assert_eq!(config.extra.get("role"), Some(&"webserver".to_string()));
    }

    #[test]
    fn test_load_hardware_config_with_ssh_host_keys() {
        let dir = setup_test_dir();
        let mac = "aa-bb-cc-dd-ee-ff";
        let hardware_dir = dir.path().join("hardware").join(mac);
        std::fs::create_dir_all(&hardware_dir).unwrap();
        std::fs::write(
            hardware_dir.join("hardware.cfg"),
            "hostname=server01\n\
             base64_ssh_host_key_ed25519_public=QUFBQUI=\n\
             base64_ssh_host_key_ed25519_private=QkJCQkI=\n",
        )
        .unwrap();

        let service = HardwareService::new(dir.path().to_path_buf());
        let config = service.load(mac).unwrap();

        assert_eq!(config.hostname, "server01");
        assert_eq!(config.base64_ssh_host_key_ed25519_public, Some("QUFBQUI=".to_string()));
        assert_eq!(config.base64_ssh_host_key_ed25519_private, Some("QkJCQkI=".to_string()));
        assert_eq!(config.base64_ssh_host_key_ecdsa_public, None);
        assert_eq!(config.base64_ssh_host_key_rsa_public, None);
    }

    #[test]
    fn test_load_hardware_config_not_found() {
        let dir = setup_test_dir();
        let service = HardwareService::new(dir.path().to_path_buf());

        let result = service.load("aa-bb-cc-dd-ee-ff");
        assert!(matches!(result, Err(AppError::HardwareConfigNotFound { .. })));
    }

    #[test]
    fn test_load_hardware_config_missing_hostname() {
        let dir = setup_test_dir();
        let mac = "aa-bb-cc-dd-ee-ff";
        let hardware_dir = dir.path().join("hardware").join(mac);
        std::fs::create_dir_all(&hardware_dir).unwrap();
        std::fs::write(hardware_dir.join("hardware.cfg"), "role=webserver\n").unwrap();

        let service = HardwareService::new(dir.path().to_path_buf());
        let result = service.load(mac);

        assert!(matches!(result, Err(AppError::ConfigParse { .. })));
    }
}
