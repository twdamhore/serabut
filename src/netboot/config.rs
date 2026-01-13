//! Netboot image configuration.
//!
//! Defines configurations for different operating systems and versions.

use std::fmt;

/// Configuration for a netboot image source.
#[derive(Debug, Clone)]
pub struct NetbootConfig {
    /// Human-readable name (e.g., "Ubuntu 24.04 LTS")
    pub name: String,
    /// Short identifier (e.g., "ubuntu-24.04")
    pub id: String,
    /// Base URL for downloads
    pub base_url: String,
    /// Netboot archive filename
    pub archive_filename: String,
    /// SHA256SUMS filename (None if not available)
    pub sha256sums_filename: Option<String>,
    /// Expected SHA256 hash (if sha256sums_filename is None)
    pub expected_sha256: Option<String>,
    /// Boot file for BIOS clients
    pub boot_file_bios: String,
    /// Boot file for EFI clients
    pub boot_file_efi: String,
    /// Architecture
    pub arch: NetbootArch,
}

/// Supported architectures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetbootArch {
    Amd64,
    Arm64,
}

impl fmt::Display for NetbootArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetbootArch::Amd64 => write!(f, "amd64"),
            NetbootArch::Arm64 => write!(f, "arm64"),
        }
    }
}

impl NetbootConfig {
    /// Get the full URL for the archive.
    pub fn archive_url(&self) -> String {
        format!("{}/{}", self.base_url, self.archive_filename)
    }

    /// Get the full URL for SHA256SUMS (if available).
    pub fn sha256sums_url(&self) -> Option<String> {
        self.sha256sums_filename
            .as_ref()
            .map(|f| format!("{}/{}", self.base_url, f))
    }
}

/// Pre-defined netboot configurations.
pub struct NetbootConfigs;

impl NetbootConfigs {
    /// Ubuntu 24.04 LTS (Noble Numbat) - amd64
    pub fn ubuntu_24_04() -> NetbootConfig {
        NetbootConfig {
            name: "Ubuntu 24.04 LTS (Noble Numbat)".to_string(),
            id: "ubuntu-24.04".to_string(),
            base_url: "https://releases.ubuntu.com/24.04".to_string(),
            archive_filename: "ubuntu-24.04.2-netboot-amd64.tar.gz".to_string(),
            sha256sums_filename: Some("SHA256SUMS".to_string()),
            expected_sha256: None,
            boot_file_bios: "pxelinux.0".to_string(),
            boot_file_efi: "grubnetx64.efi.signed".to_string(),
            arch: NetbootArch::Amd64,
        }
    }

    /// Ubuntu 22.04 LTS (Jammy Jellyfish) - amd64
    pub fn ubuntu_22_04() -> NetbootConfig {
        NetbootConfig {
            name: "Ubuntu 22.04 LTS (Jammy Jellyfish)".to_string(),
            id: "ubuntu-22.04".to_string(),
            base_url: "https://releases.ubuntu.com/22.04".to_string(),
            archive_filename: "ubuntu-22.04.5-netboot-amd64.tar.gz".to_string(),
            sha256sums_filename: Some("SHA256SUMS".to_string()),
            expected_sha256: None,
            boot_file_bios: "pxelinux.0".to_string(),
            boot_file_efi: "grubnetx64.efi.signed".to_string(),
            arch: NetbootArch::Amd64,
        }
    }

    /// Rocky Linux 9 - amd64
    /// Note: Rocky uses a different netboot structure
    pub fn rocky_9() -> NetbootConfig {
        NetbootConfig {
            name: "Rocky Linux 9".to_string(),
            id: "rocky-9".to_string(),
            base_url: "https://download.rockylinux.org/pub/rocky/9/BaseOS/x86_64/os/images/pxeboot".to_string(),
            archive_filename: "initrd.img".to_string(), // Rocky doesn't use tar.gz
            sha256sums_filename: None, // Would need separate handling
            expected_sha256: None,
            boot_file_bios: "pxelinux.0".to_string(),
            boot_file_efi: "grubx64.efi".to_string(),
            arch: NetbootArch::Amd64,
        }
    }

    /// AlmaLinux 9 - amd64
    pub fn alma_9() -> NetbootConfig {
        NetbootConfig {
            name: "AlmaLinux 9".to_string(),
            id: "alma-9".to_string(),
            base_url: "https://repo.almalinux.org/almalinux/9/BaseOS/x86_64/os/images/pxeboot".to_string(),
            archive_filename: "initrd.img".to_string(),
            sha256sums_filename: None,
            expected_sha256: None,
            boot_file_bios: "pxelinux.0".to_string(),
            boot_file_efi: "grubx64.efi".to_string(),
            arch: NetbootArch::Amd64,
        }
    }

    /// Debian 12 (Bookworm) - amd64
    pub fn debian_12() -> NetbootConfig {
        NetbootConfig {
            name: "Debian 12 (Bookworm)".to_string(),
            id: "debian-12".to_string(),
            base_url: "https://deb.debian.org/debian/dists/bookworm/main/installer-amd64/current/images/netboot".to_string(),
            archive_filename: "netboot.tar.gz".to_string(),
            sha256sums_filename: Some("SHA256SUMS".to_string()),
            expected_sha256: None,
            boot_file_bios: "pxelinux.0".to_string(),
            boot_file_efi: "grubnetx64.efi.signed".to_string(),
            arch: NetbootArch::Amd64,
        }
    }

    /// Get configuration by ID.
    pub fn get(id: &str) -> Option<NetbootConfig> {
        match id {
            "ubuntu-24.04" | "ubuntu" => Some(Self::ubuntu_24_04()),
            "ubuntu-22.04" => Some(Self::ubuntu_22_04()),
            "rocky-9" | "rocky" => Some(Self::rocky_9()),
            "alma-9" | "alma" => Some(Self::alma_9()),
            "debian-12" | "debian" => Some(Self::debian_12()),
            _ => None,
        }
    }

    /// List all available configurations.
    pub fn list() -> Vec<NetbootConfig> {
        vec![
            Self::ubuntu_24_04(),
            Self::ubuntu_22_04(),
            Self::debian_12(),
            Self::rocky_9(),
            Self::alma_9(),
        ]
    }

    /// List available configuration IDs.
    pub fn available_ids() -> Vec<&'static str> {
        vec![
            "ubuntu-24.04",
            "ubuntu-22.04",
            "debian-12",
            "rocky-9",
            "alma-9",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ubuntu_24_04_config() {
        let config = NetbootConfigs::ubuntu_24_04();
        assert_eq!(config.id, "ubuntu-24.04");
        assert!(config.archive_url().contains("ubuntu-24.04"));
        assert!(config.sha256sums_url().is_some());
    }

    #[test]
    fn test_get_by_id() {
        assert!(NetbootConfigs::get("ubuntu-24.04").is_some());
        assert!(NetbootConfigs::get("ubuntu").is_some());
        assert!(NetbootConfigs::get("rocky-9").is_some());
        assert!(NetbootConfigs::get("nonexistent").is_none());
    }

    #[test]
    fn test_list() {
        let configs = NetbootConfigs::list();
        assert!(!configs.is_empty());
        assert!(configs.iter().any(|c| c.id == "ubuntu-24.04"));
    }

    #[test]
    fn test_archive_url() {
        let config = NetbootConfigs::ubuntu_24_04();
        let url = config.archive_url();
        assert!(url.starts_with("https://"));
        assert!(url.ends_with(".tar.gz"));
    }
}
