use std::collections::HashMap;
use std::path::Path;

use crate::error::AppError;
use crate::utils::normalize_mac;

#[derive(Debug)]
pub struct HardwareConfig {
    /// hostname -> (key -> value)
    entries: HashMap<String, HashMap<String, String>>,
    /// normalized MAC -> hostname
    mac_to_hostname: HashMap<String, String>,
}

impl HardwareConfig {
    pub fn load(hardware_dir: &Path) -> Result<Self, AppError> {
        let mut entries = HashMap::new();
        let mut mac_to_hostname = HashMap::new();

        if !hardware_dir.exists() {
            return Ok(HardwareConfig {
                entries,
                mac_to_hostname,
            });
        }

        let read_dir = std::fs::read_dir(hardware_dir)?;

        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("cfg") {
                continue;
            }

            let hostname = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| AppError::Config("Invalid hardware config filename".to_string()))?
                .to_string();

            let content = std::fs::read_to_string(&path)?;
            let mut hw_entry = HashMap::new();

            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }

                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim().to_string();
                    let value = value.trim().to_string();

                    if key == "mac" {
                        let normalized = normalize_mac(&value);
                        mac_to_hostname.insert(normalized.clone(), hostname.clone());
                        hw_entry.insert(key, normalized);
                    } else {
                        hw_entry.insert(key, value);
                    }
                }
            }

            entries.insert(hostname, hw_entry);
        }

        Ok(HardwareConfig {
            entries,
            mac_to_hostname,
        })
    }

    pub fn get(&self, hostname: &str) -> Option<&HashMap<String, String>> {
        self.entries.get(hostname)
    }

    pub fn hostname_by_mac(&self, mac: &str) -> Option<&str> {
        let normalized = normalize_mac(mac);
        self.mac_to_hostname.get(&normalized).map(|s| s.as_str())
    }
}
