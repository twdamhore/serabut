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
    /// Netboot archive filename (may be auto-discovered for Ubuntu)
    pub archive_filename: String,
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
}

// =============================================================================
// VERSION REGISTRIES - Add new versions here (just one line each!)
// =============================================================================

/// Ubuntu versions: (version, codename)
const UBUNTU_VERSIONS: &[(&str, &str)] = &[
    ("24.04", "Noble Numbat"),
    // ("26.04", "Plucky Puffin"),  // Add future versions here
];

/// Debian versions: (version, codename)
const DEBIAN_VERSIONS: &[(&str, &str)] = &[
    ("12", "bookworm"),
    // ("13", "trixie"),  // Add future versions here
];

/// Rocky Linux versions: just the major version number
const ROCKY_VERSIONS: &[&str] = &[
    "9",
    "10",
    // "11",  // Add future versions here
];

/// AlmaLinux versions: just the major version number
const ALMA_VERSIONS: &[&str] = &[
    "9",
    "10",
    // "11",  // Add future versions here
];

// =============================================================================

/// Pre-defined netboot configurations.
pub struct NetbootConfigs;

impl NetbootConfigs {
    /// Create Ubuntu LTS config for any version.
    pub fn ubuntu(version: &str, codename: &str) -> NetbootConfig {
        NetbootConfig {
            name: format!("Ubuntu {} LTS ({})", version, codename),
            id: format!("ubuntu-{}", version),
            base_url: format!("https://releases.ubuntu.com/{}", version),
            archive_filename: format!("ubuntu-{}-netboot-amd64.tar.gz", version),
            boot_file_bios: "amd64/pxelinux.0".to_string(),
            boot_file_efi: "amd64/grubx64.efi".to_string(),
            arch: NetbootArch::Amd64,
        }
    }

    /// Create Debian config for any version.
    pub fn debian(version: &str, codename: &str) -> NetbootConfig {
        NetbootConfig {
            name: format!("Debian {} ({})", version, codename.chars().next().unwrap().to_uppercase().collect::<String>() + &codename[1..]),
            id: format!("debian-{}", version),
            base_url: format!("https://deb.debian.org/debian/dists/{}/main/installer-amd64/current/images/netboot", codename),
            archive_filename: "netboot.tar.gz".to_string(),
            boot_file_bios: "pxelinux.0".to_string(),
            boot_file_efi: "grubnetx64.efi.signed".to_string(),
            arch: NetbootArch::Amd64,
        }
    }

    /// Create Rocky Linux config for any version.
    pub fn rocky(version: &str) -> NetbootConfig {
        NetbootConfig {
            name: format!("Rocky Linux {}", version),
            id: format!("rocky-{}", version),
            base_url: format!("https://download.rockylinux.org/pub/rocky/{}/BaseOS/x86_64/os/images/pxeboot", version),
            archive_filename: "initrd.img".to_string(),
            boot_file_bios: "pxelinux.0".to_string(),
            boot_file_efi: "grubx64.efi".to_string(),
            arch: NetbootArch::Amd64,
        }
    }

    /// Create AlmaLinux config for any version.
    pub fn alma(version: &str) -> NetbootConfig {
        NetbootConfig {
            name: format!("AlmaLinux {}", version),
            id: format!("alma-{}", version),
            base_url: format!("https://repo.almalinux.org/almalinux/{}/BaseOS/x86_64/os/images/pxeboot", version),
            archive_filename: "initrd.img".to_string(),
            boot_file_bios: "pxelinux.0".to_string(),
            boot_file_efi: "grubx64.efi".to_string(),
            arch: NetbootArch::Amd64,
        }
    }

    // Convenience aliases for specific versions
    pub fn ubuntu_24_04() -> NetbootConfig { Self::ubuntu("24.04", "Noble Numbat") }
    pub fn debian_12() -> NetbootConfig { Self::debian("12", "bookworm") }
    pub fn rocky_9() -> NetbootConfig { Self::rocky("9") }
    pub fn rocky_10() -> NetbootConfig { Self::rocky("10") }
    pub fn alma_9() -> NetbootConfig { Self::alma("9") }
    pub fn alma_10() -> NetbootConfig { Self::alma("10") }

    /// Get configuration by ID.
    pub fn get(id: &str) -> Option<NetbootConfig> {
        // Check Ubuntu versions
        for (version, codename) in UBUNTU_VERSIONS {
            if id == format!("ubuntu-{}", version) {
                return Some(Self::ubuntu(version, codename));
            }
        }
        if id == "ubuntu" {
            if let Some((v, c)) = UBUNTU_VERSIONS.last() {
                return Some(Self::ubuntu(v, c));
            }
        }

        // Check Debian versions
        for (version, codename) in DEBIAN_VERSIONS {
            if id == format!("debian-{}", version) {
                return Some(Self::debian(version, codename));
            }
        }
        if id == "debian" {
            if let Some((v, c)) = DEBIAN_VERSIONS.last() {
                return Some(Self::debian(v, c));
            }
        }

        // Check Rocky versions
        for version in ROCKY_VERSIONS {
            if id == format!("rocky-{}", version) {
                return Some(Self::rocky(version));
            }
        }
        if id == "rocky" {
            if let Some(v) = ROCKY_VERSIONS.last() {
                return Some(Self::rocky(v));
            }
        }

        // Check Alma versions
        for version in ALMA_VERSIONS {
            if id == format!("alma-{}", version) {
                return Some(Self::alma(version));
            }
        }
        if id == "alma" {
            if let Some(v) = ALMA_VERSIONS.last() {
                return Some(Self::alma(v));
            }
        }

        None
    }

    /// List all available configurations.
    pub fn list() -> Vec<NetbootConfig> {
        let mut configs = Vec::new();
        for (v, c) in UBUNTU_VERSIONS { configs.push(Self::ubuntu(v, c)); }
        for (v, c) in DEBIAN_VERSIONS { configs.push(Self::debian(v, c)); }
        for v in ROCKY_VERSIONS { configs.push(Self::rocky(v)); }
        for v in ALMA_VERSIONS { configs.push(Self::alma(v)); }
        configs
    }

    /// List available configuration IDs.
    pub fn available_ids() -> Vec<String> {
        let mut ids = Vec::new();
        for (v, _) in UBUNTU_VERSIONS { ids.push(format!("ubuntu-{}", v)); }
        for (v, _) in DEBIAN_VERSIONS { ids.push(format!("debian-{}", v)); }
        for v in ROCKY_VERSIONS { ids.push(format!("rocky-{}", v)); }
        for v in ALMA_VERSIONS { ids.push(format!("alma-{}", v)); }
        ids
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
        assert_eq!(config.boot_file_bios, "amd64/pxelinux.0");
        assert_eq!(config.boot_file_efi, "amd64/grubx64.efi");
        assert_eq!(config.arch, NetbootArch::Amd64);
    }

    #[test]
    fn test_debian_12_config() {
        let config = NetbootConfigs::debian_12();
        assert_eq!(config.id, "debian-12");
        assert_eq!(config.name, "Debian 12 (Bookworm)");
        assert!(config.archive_url().contains("bookworm"));
        assert_eq!(config.boot_file_efi, "grubnetx64.efi.signed");
    }

    #[test]
    fn test_rocky_9_config() {
        let config = NetbootConfigs::rocky_9();
        assert_eq!(config.id, "rocky-9");
        assert_eq!(config.name, "Rocky Linux 9");
        assert!(config.archive_url().contains("rocky/9"));
        assert_eq!(config.boot_file_efi, "grubx64.efi");
    }

    #[test]
    fn test_rocky_10_config() {
        let config = NetbootConfigs::rocky_10();
        assert_eq!(config.id, "rocky-10");
        assert_eq!(config.name, "Rocky Linux 10");
        assert!(config.archive_url().contains("rocky/10"));
        assert_eq!(config.boot_file_efi, "grubx64.efi");
    }

    #[test]
    fn test_alma_9_config() {
        let config = NetbootConfigs::alma_9();
        assert_eq!(config.id, "alma-9");
        assert_eq!(config.name, "AlmaLinux 9");
        assert!(config.archive_url().contains("almalinux/9"));
        assert_eq!(config.boot_file_efi, "grubx64.efi");
    }

    #[test]
    fn test_alma_10_config() {
        let config = NetbootConfigs::alma_10();
        assert_eq!(config.id, "alma-10");
        assert_eq!(config.name, "AlmaLinux 10");
        assert!(config.archive_url().contains("almalinux/10"));
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
        assert!(ids.iter().any(|id| id == "ubuntu-24.04"));
        assert!(ids.iter().any(|id| id == "debian-12"));
        assert!(ids.iter().any(|id| id == "rocky-9"));
        assert!(ids.iter().any(|id| id == "rocky-10"));
        assert!(ids.iter().any(|id| id == "alma-9"));
        assert!(ids.iter().any(|id| id == "alma-10"));
    }

    #[test]
    fn test_archive_url() {
        let config = NetbootConfigs::ubuntu_24_04();
        let url = config.archive_url();
        assert!(url.starts_with("https://"));
        assert!(url.ends_with(".tar.gz"));
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
