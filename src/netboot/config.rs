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
            archive_filename: "ubuntu-24.04.3-netboot-amd64.tar.gz".to_string(),
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

    /// Rocky Linux 10 - amd64
    pub fn rocky_10() -> NetbootConfig {
        NetbootConfig {
            name: "Rocky Linux 10".to_string(),
            id: "rocky-10".to_string(),
            base_url: "https://download.rockylinux.org/pub/rocky/10/BaseOS/x86_64/os/images/pxeboot".to_string(),
            archive_filename: "initrd.img".to_string(),
            sha256sums_filename: None,
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

    /// AlmaLinux 10 - amd64
    pub fn alma_10() -> NetbootConfig {
        NetbootConfig {
            name: "AlmaLinux 10".to_string(),
            id: "alma-10".to_string(),
            base_url: "https://repo.almalinux.org/almalinux/10/BaseOS/x86_64/os/images/pxeboot".to_string(),
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
            "rocky-9" => Some(Self::rocky_9()),
            "rocky-10" | "rocky" => Some(Self::rocky_10()),
            "alma-9" => Some(Self::alma_9()),
            "alma-10" | "alma" => Some(Self::alma_10()),
            "debian-12" | "debian" => Some(Self::debian_12()),
            _ => None,
        }
    }

    /// List all available configurations.
    pub fn list() -> Vec<NetbootConfig> {
        vec![
            Self::ubuntu_24_04(),
            Self::debian_12(),
            Self::rocky_9(),
            Self::rocky_10(),
            Self::alma_9(),
            Self::alma_10(),
        ]
    }

    /// List available configuration IDs.
    pub fn available_ids() -> Vec<&'static str> {
        vec![
            "ubuntu-24.04",
            "debian-12",
            "rocky-9",
            "rocky-10",
            "alma-9",
            "alma-10",
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
        assert_eq!(config.name, "Ubuntu 24.04 LTS (Noble Numbat)");
        assert!(config.archive_url().contains("ubuntu-24.04"));
        assert!(config.sha256sums_url().is_some());
        assert_eq!(config.boot_file_bios, "pxelinux.0");
        assert_eq!(config.boot_file_efi, "grubnetx64.efi.signed");
        assert_eq!(config.arch, NetbootArch::Amd64);
    }

    #[test]
    fn test_debian_12_config() {
        let config = NetbootConfigs::debian_12();
        assert_eq!(config.id, "debian-12");
        assert_eq!(config.name, "Debian 12 (Bookworm)");
        assert!(config.archive_url().contains("bookworm"));
        assert!(config.sha256sums_url().is_some());
        assert_eq!(config.boot_file_efi, "grubnetx64.efi.signed");
    }

    #[test]
    fn test_rocky_9_config() {
        let config = NetbootConfigs::rocky_9();
        assert_eq!(config.id, "rocky-9");
        assert_eq!(config.name, "Rocky Linux 9");
        assert!(config.archive_url().contains("rocky/9"));
        assert!(config.sha256sums_url().is_none());
        assert_eq!(config.boot_file_efi, "grubx64.efi");
    }

    #[test]
    fn test_rocky_10_config() {
        let config = NetbootConfigs::rocky_10();
        assert_eq!(config.id, "rocky-10");
        assert_eq!(config.name, "Rocky Linux 10");
        assert!(config.archive_url().contains("rocky/10"));
        assert!(config.sha256sums_url().is_none());
        assert_eq!(config.boot_file_efi, "grubx64.efi");
    }

    #[test]
    fn test_alma_9_config() {
        let config = NetbootConfigs::alma_9();
        assert_eq!(config.id, "alma-9");
        assert_eq!(config.name, "AlmaLinux 9");
        assert!(config.archive_url().contains("almalinux/9"));
        assert!(config.sha256sums_url().is_none());
        assert_eq!(config.boot_file_efi, "grubx64.efi");
    }

    #[test]
    fn test_alma_10_config() {
        let config = NetbootConfigs::alma_10();
        assert_eq!(config.id, "alma-10");
        assert_eq!(config.name, "AlmaLinux 10");
        assert!(config.archive_url().contains("almalinux/10"));
        assert!(config.sha256sums_url().is_none());
        assert_eq!(config.boot_file_efi, "grubx64.efi");
    }

    #[test]
    fn test_get_by_id() {
        assert!(NetbootConfigs::get("ubuntu-24.04").is_some());
        assert!(NetbootConfigs::get("ubuntu").is_some());
        assert!(NetbootConfigs::get("rocky-9").is_some());
        assert!(NetbootConfigs::get("rocky-10").is_some());
        assert!(NetbootConfigs::get("rocky").is_some());
        assert!(NetbootConfigs::get("alma-9").is_some());
        assert!(NetbootConfigs::get("alma-10").is_some());
        assert!(NetbootConfigs::get("alma").is_some());
        assert!(NetbootConfigs::get("debian-12").is_some());
        assert!(NetbootConfigs::get("debian").is_some());
        assert!(NetbootConfigs::get("nonexistent").is_none());
    }

    #[test]
    fn test_get_aliases() {
        // Test that aliases map to latest versions
        let ubuntu = NetbootConfigs::get("ubuntu").unwrap();
        assert_eq!(ubuntu.id, "ubuntu-24.04");

        let rocky = NetbootConfigs::get("rocky").unwrap();
        assert_eq!(rocky.id, "rocky-10");

        let alma = NetbootConfigs::get("alma").unwrap();
        assert_eq!(alma.id, "alma-10");

        let debian = NetbootConfigs::get("debian").unwrap();
        assert_eq!(debian.id, "debian-12");
    }

    #[test]
    fn test_list() {
        let configs = NetbootConfigs::list();
        assert_eq!(configs.len(), 6);
        assert!(configs.iter().any(|c| c.id == "ubuntu-24.04"));
        assert!(configs.iter().any(|c| c.id == "debian-12"));
        assert!(configs.iter().any(|c| c.id == "rocky-9"));
        assert!(configs.iter().any(|c| c.id == "rocky-10"));
        assert!(configs.iter().any(|c| c.id == "alma-9"));
        assert!(configs.iter().any(|c| c.id == "alma-10"));
    }

    #[test]
    fn test_available_ids() {
        let ids = NetbootConfigs::available_ids();
        assert_eq!(ids.len(), 6);
        assert!(ids.contains(&"ubuntu-24.04"));
        assert!(ids.contains(&"debian-12"));
        assert!(ids.contains(&"rocky-9"));
        assert!(ids.contains(&"rocky-10"));
        assert!(ids.contains(&"alma-9"));
        assert!(ids.contains(&"alma-10"));
    }

    #[test]
    fn test_archive_url() {
        let config = NetbootConfigs::ubuntu_24_04();
        let url = config.archive_url();
        assert!(url.starts_with("https://"));
        assert!(url.ends_with(".tar.gz"));
    }

    #[test]
    fn test_sha256sums_url_some() {
        let config = NetbootConfigs::ubuntu_24_04();
        let url = config.sha256sums_url();
        assert!(url.is_some());
        assert!(url.unwrap().ends_with("SHA256SUMS"));
    }

    #[test]
    fn test_sha256sums_url_none() {
        let config = NetbootConfigs::rocky_9();
        assert!(config.sha256sums_url().is_none());
    }

    #[test]
    fn test_netboot_arch_display() {
        assert_eq!(format!("{}", NetbootArch::Amd64), "amd64");
        assert_eq!(format!("{}", NetbootArch::Arm64), "arm64");
    }

    #[test]
    fn test_netboot_arch_equality() {
        assert_eq!(NetbootArch::Amd64, NetbootArch::Amd64);
        assert_ne!(NetbootArch::Amd64, NetbootArch::Arm64);
    }

    #[test]
    fn test_netboot_config_clone() {
        let config = NetbootConfigs::ubuntu_24_04();
        let cloned = config.clone();
        assert_eq!(config.id, cloned.id);
        assert_eq!(config.name, cloned.name);
        assert_eq!(config.base_url, cloned.base_url);
    }

    #[test]
    fn test_all_configs_have_valid_urls() {
        for config in NetbootConfigs::list() {
            let url = config.archive_url();
            assert!(url.starts_with("https://"), "Config {} has invalid URL", config.id);
        }
    }

    #[test]
    fn test_all_configs_have_boot_files() {
        for config in NetbootConfigs::list() {
            assert!(!config.boot_file_bios.is_empty(), "Config {} missing BIOS boot file", config.id);
            assert!(!config.boot_file_efi.is_empty(), "Config {} missing EFI boot file", config.id);
        }
    }
}
