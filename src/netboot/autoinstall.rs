//! Autoinstall configuration for Ubuntu cloud-init.
//!
//! Generates bootloader configurations with autoinstall parameters.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::info;

/// Autoinstall configuration.
#[derive(Debug, Clone)]
pub struct AutoinstallConfig {
    /// URL for cloud-init datasource (e.g., "http://192.168.1.100:8080/").
    pub datasource_url: String,
    /// Path to user-data file (optional, for serving).
    pub user_data_path: Option<PathBuf>,
    /// Path to meta-data file (optional).
    pub meta_data_path: Option<PathBuf>,
}

impl AutoinstallConfig {
    /// Create a new autoinstall configuration.
    pub fn new(datasource_url: impl Into<String>) -> Self {
        Self {
            datasource_url: datasource_url.into(),
            user_data_path: None,
            meta_data_path: None,
        }
    }

    /// Set user-data file path.
    pub fn with_user_data(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_data_path = Some(path.into());
        self
    }

    /// Set meta-data file path.
    pub fn with_meta_data(mut self, path: impl Into<PathBuf>) -> Self {
        self.meta_data_path = Some(path.into());
        self
    }

    /// Get kernel parameters for autoinstall.
    pub fn kernel_params(&self) -> String {
        format!(
            "autoinstall ds=nocloud-net;s={}",
            self.datasource_url
        )
    }
}

/// Bootloader configuration generator.
pub struct BootloaderConfigGenerator {
    /// TFTP root directory.
    tftp_root: PathBuf,
    /// Autoinstall configuration.
    autoinstall: Option<AutoinstallConfig>,
    /// HTTP server URL for serving kernel/initrd (faster than TFTP).
    http_boot_url: Option<String>,
}

impl BootloaderConfigGenerator {
    /// Create a new bootloader config generator.
    pub fn new<P: AsRef<Path>>(tftp_root: P) -> Self {
        Self {
            tftp_root: tftp_root.as_ref().to_path_buf(),
            autoinstall: None,
            http_boot_url: None,
        }
    }

    /// Set autoinstall configuration.
    pub fn with_autoinstall(mut self, config: AutoinstallConfig) -> Self {
        self.autoinstall = Some(config);
        self
    }

    /// Set HTTP boot URL for faster kernel/initrd transfers.
    /// Format: "http://ip:port" (e.g., "http://192.168.1.100:8080")
    pub fn with_http_boot(mut self, url: impl Into<String>) -> Self {
        self.http_boot_url = Some(url.into());
        self
    }

    /// Generate all bootloader configurations.
    pub fn generate(&self) -> Result<()> {
        self.generate_grub_config()?;
        self.generate_syslinux_config()?;
        Ok(())
    }

    /// Generate GRUB configuration for UEFI boot.
    pub fn generate_grub_config(&self) -> Result<()> {
        let config = self.grub_config_content();

        // Write to multiple locations to ensure GRUB finds our config
        // Ubuntu's GRUB looks in amd64/grub/, generic GRUB looks in grub/
        let locations = [
            self.tftp_root.join("grub"),
            self.tftp_root.join("amd64").join("grub"),
        ];

        for grub_dir in &locations {
            if grub_dir.exists() || fs::create_dir_all(grub_dir).is_ok() {
                let grub_cfg_path = grub_dir.join("grub.cfg");
                if let Ok(mut file) = fs::File::create(&grub_cfg_path) {
                    if file.write_all(config.as_bytes()).is_ok() {
                        info!("Generated GRUB config: {:?}", grub_cfg_path);
                    }
                }
            }
        }

        Ok(())
    }

    /// Generate syslinux/pxelinux configuration for BIOS boot.
    pub fn generate_syslinux_config(&self) -> Result<()> {
        let pxe_dir = self.tftp_root.join("pxelinux.cfg");
        if !pxe_dir.exists() {
            fs::create_dir_all(&pxe_dir)
                .context("Failed to create pxelinux.cfg directory")?;
        }

        let default_path = pxe_dir.join("default");
        let config = self.syslinux_config_content();

        let mut file = fs::File::create(&default_path)
            .with_context(|| format!("Failed to create {:?}", default_path))?;
        file.write_all(config.as_bytes())?;

        info!("Generated syslinux config: {:?}", default_path);
        Ok(())
    }

    /// Generate GRUB configuration content.
    fn grub_config_content(&self) -> String {
        let extra_params = self.autoinstall
            .as_ref()
            .map(|a| format!(" {}", a.kernel_params()))
            .unwrap_or_default();

        // Use HTTP for kernel/initrd if configured (much faster than TFTP)
        let (linux_path, initrd_path) = if let Some(ref url) = self.http_boot_url {
            // Parse URL to get host:port for GRUB's (http,host:port) syntax
            // URL format: "http://192.168.1.100:8080"
            let host_port = url
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .trim_end_matches('/');
            (
                format!("(http,{})/linux", host_port),
                format!("(http,{})/initrd", host_port),
            )
        } else {
            // Fall back to TFTP (relative paths)
            ("/linux".to_string(), "/initrd".to_string())
        };

        let boot_method = if self.http_boot_url.is_some() { " via HTTP" } else { "" };
        let autoinstall_label = if self.autoinstall.is_some() { " (Autoinstall)" } else { "" };

        format!(r#"# GRUB configuration generated by serabut
# Ubuntu autoinstall PXE boot{boot_method}

# Boot menu settings
set default=0
set timeout=0

# Main install option (default)
menuentry "Ubuntu Server{autoinstall_label}" {{
    echo "Loading kernel{boot_method}..."
    linux {linux_path} ip=dhcp{extra_params}
    echo "Loading initrd{boot_method}..."
    initrd {initrd_path}
}}

# Safe mode with basic graphics
menuentry "Ubuntu Server{autoinstall_label} (Safe Graphics)" {{
    echo "Loading kernel{boot_method}..."
    linux {linux_path} ip=dhcp nomodeset{extra_params}
    echo "Loading initrd{boot_method}..."
    initrd {initrd_path}
}}

# Boot from local disk
menuentry "Boot from local disk" {{
    exit
}}
"#,
            boot_method = boot_method,
            autoinstall_label = autoinstall_label,
            linux_path = linux_path,
            initrd_path = initrd_path,
            extra_params = extra_params,
        )
    }

    /// Generate syslinux configuration content.
    fn syslinux_config_content(&self) -> String {
        let extra_params = self.autoinstall
            .as_ref()
            .map(|a| format!(" {}", a.kernel_params()))
            .unwrap_or_default();

        format!(r#"# Syslinux configuration generated by serabut
# Ubuntu autoinstall PXE boot

DEFAULT install
TIMEOUT 50
PROMPT 1

LABEL install
    MENU LABEL Ubuntu Server Install{}
    KERNEL casper/vmlinuz
    APPEND initrd=casper/initrd ip=dhcp{}

LABEL install-safe
    MENU LABEL Ubuntu Server Install (Safe Mode)
    KERNEL casper/vmlinuz
    APPEND initrd=casper/initrd ip=dhcp nomodeset{}
"#,
            if self.autoinstall.is_some() { " (Autoinstall)" } else { "" },
            extra_params,
            extra_params,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autoinstall_config_new() {
        let config = AutoinstallConfig::new("http://192.168.1.100:8080/");
        assert_eq!(config.datasource_url, "http://192.168.1.100:8080/");
        assert!(config.user_data_path.is_none());
    }

    #[test]
    fn test_autoinstall_config_with_user_data() {
        let config = AutoinstallConfig::new("http://test/")
            .with_user_data("/path/to/user-data");
        assert_eq!(config.user_data_path, Some(PathBuf::from("/path/to/user-data")));
    }

    #[test]
    fn test_autoinstall_kernel_params() {
        let config = AutoinstallConfig::new("http://192.168.1.100:8080/");
        let params = config.kernel_params();
        assert_eq!(params, "autoinstall ds=nocloud-net;s=http://192.168.1.100:8080/");
    }

    #[test]
    fn test_bootloader_generator_new() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        assert_eq!(gen.tftp_root, PathBuf::from("/tmp/tftp"));
        assert!(gen.autoinstall.is_none());
    }

    #[test]
    fn test_bootloader_generator_with_autoinstall() {
        let config = AutoinstallConfig::new("http://test/");
        let gen = BootloaderConfigGenerator::new("/tmp/tftp")
            .with_autoinstall(config);
        assert!(gen.autoinstall.is_some());
    }

    #[test]
    fn test_grub_config_without_autoinstall() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.grub_config_content();
        assert!(content.contains("menuentry"));
        assert!(content.contains("/linux"));
        assert!(content.contains("/initrd"));
        // Should not contain autoinstall kernel parameter
        assert!(!content.contains("ds=nocloud-net"));
    }

    #[test]
    fn test_grub_config_with_autoinstall() {
        let config = AutoinstallConfig::new("http://192.168.1.100:8080/");
        let gen = BootloaderConfigGenerator::new("/tmp/tftp")
            .with_autoinstall(config);
        let content = gen.grub_config_content();
        assert!(content.contains("autoinstall"));
        assert!(content.contains("ds=nocloud-net"));
        assert!(content.contains("http://192.168.1.100:8080/"));
    }

    #[test]
    fn test_syslinux_config_without_autoinstall() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.syslinux_config_content();
        assert!(content.contains("LABEL install"));
        assert!(content.contains("casper/vmlinuz"));
        // Should not contain autoinstall kernel parameter
        assert!(!content.contains("ds=nocloud-net"));
    }

    #[test]
    fn test_syslinux_config_with_autoinstall() {
        let config = AutoinstallConfig::new("http://192.168.1.100:8080/");
        let gen = BootloaderConfigGenerator::new("/tmp/tftp")
            .with_autoinstall(config);
        let content = gen.syslinux_config_content();
        assert!(content.contains("autoinstall"));
        assert!(content.contains("ds=nocloud-net"));
        assert!(content.contains("http://192.168.1.100:8080/"));
    }

    #[test]
    fn test_autoinstall_config_with_meta_data() {
        let config = AutoinstallConfig::new("http://test/")
            .with_meta_data("/path/to/meta-data");
        assert_eq!(config.meta_data_path, Some(PathBuf::from("/path/to/meta-data")));
    }

    #[test]
    fn test_autoinstall_config_builder_chain() {
        let config = AutoinstallConfig::new("http://example.com/")
            .with_user_data("/user-data")
            .with_meta_data("/meta-data");
        assert_eq!(config.datasource_url, "http://example.com/");
        assert_eq!(config.user_data_path, Some(PathBuf::from("/user-data")));
        assert_eq!(config.meta_data_path, Some(PathBuf::from("/meta-data")));
    }

    #[test]
    fn test_kernel_params_different_urls() {
        let config = AutoinstallConfig::new("http://10.0.0.1:3000/cloud-init/");
        let params = config.kernel_params();
        assert_eq!(params, "autoinstall ds=nocloud-net;s=http://10.0.0.1:3000/cloud-init/");
    }

    #[test]
    fn test_grub_config_contains_timeout() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.grub_config_content();
        assert!(content.contains("set timeout=0"));
    }

    #[test]
    fn test_grub_config_contains_default() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.grub_config_content();
        assert!(content.contains("set default=0"));
    }

    #[test]
    fn test_grub_config_contains_safe_mode() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.grub_config_content();
        assert!(content.contains("Safe Graphics"));
        assert!(content.contains("nomodeset"));
    }

    #[test]
    fn test_grub_config_contains_ip_dhcp() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.grub_config_content();
        assert!(content.contains("ip=dhcp"));
    }

    #[test]
    fn test_syslinux_config_contains_timeout() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.syslinux_config_content();
        assert!(content.contains("TIMEOUT 50"));
    }

    #[test]
    fn test_syslinux_config_contains_default() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.syslinux_config_content();
        assert!(content.contains("DEFAULT install"));
    }

    #[test]
    fn test_syslinux_config_contains_prompt() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.syslinux_config_content();
        assert!(content.contains("PROMPT 1"));
    }

    #[test]
    fn test_syslinux_config_contains_kernel() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.syslinux_config_content();
        assert!(content.contains("KERNEL casper/vmlinuz"));
    }

    #[test]
    fn test_syslinux_config_contains_append() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.syslinux_config_content();
        assert!(content.contains("APPEND initrd=casper/initrd"));
    }

    #[test]
    fn test_syslinux_config_safe_mode_label() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp");
        let content = gen.syslinux_config_content();
        assert!(content.contains("LABEL install-safe"));
        assert!(content.contains("MENU LABEL Ubuntu Server Install (Safe Mode)"));
    }

    #[test]
    fn test_grub_config_autoinstall_menu_label() {
        let config = AutoinstallConfig::new("http://test/");
        let gen = BootloaderConfigGenerator::new("/tmp/tftp")
            .with_autoinstall(config);
        let content = gen.grub_config_content();
        assert!(content.contains("(Autoinstall)"));
    }

    #[test]
    fn test_syslinux_config_autoinstall_menu_label() {
        let config = AutoinstallConfig::new("http://test/");
        let gen = BootloaderConfigGenerator::new("/tmp/tftp")
            .with_autoinstall(config);
        let content = gen.syslinux_config_content();
        assert!(content.contains("(Autoinstall)"));
    }

    #[test]
    fn test_generate_grub_config_creates_file() {
        let temp_dir = std::env::temp_dir().join("serabut_test_grub");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let gen = BootloaderConfigGenerator::new(&temp_dir);
        let result = gen.generate_grub_config();
        assert!(result.is_ok());

        let grub_cfg = temp_dir.join("grub").join("grub.cfg");
        assert!(grub_cfg.exists());

        let content = std::fs::read_to_string(&grub_cfg).unwrap();
        assert!(content.contains("menuentry"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_generate_syslinux_config_creates_file() {
        let temp_dir = std::env::temp_dir().join("serabut_test_syslinux");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let gen = BootloaderConfigGenerator::new(&temp_dir);
        let result = gen.generate_syslinux_config();
        assert!(result.is_ok());

        let default_cfg = temp_dir.join("pxelinux.cfg").join("default");
        assert!(default_cfg.exists());

        let content = std::fs::read_to_string(&default_cfg).unwrap();
        assert!(content.contains("LABEL install"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_generate_creates_both_configs() {
        let temp_dir = std::env::temp_dir().join("serabut_test_both");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let gen = BootloaderConfigGenerator::new(&temp_dir);
        let result = gen.generate();
        assert!(result.is_ok());

        assert!(temp_dir.join("grub").join("grub.cfg").exists());
        assert!(temp_dir.join("pxelinux.cfg").join("default").exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_generate_with_autoinstall() {
        let temp_dir = std::env::temp_dir().join("serabut_test_autoinstall");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let config = AutoinstallConfig::new("http://192.168.1.100:8080/");
        let gen = BootloaderConfigGenerator::new(&temp_dir)
            .with_autoinstall(config);
        let result = gen.generate();
        assert!(result.is_ok());

        let grub_content = std::fs::read_to_string(temp_dir.join("grub").join("grub.cfg")).unwrap();
        assert!(grub_content.contains("ds=nocloud-net"));
        assert!(grub_content.contains("http://192.168.1.100:8080/"));

        let syslinux_content = std::fs::read_to_string(temp_dir.join("pxelinux.cfg").join("default")).unwrap();
        assert!(syslinux_content.contains("ds=nocloud-net"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_bootloader_generator_tftp_root_path() {
        let gen = BootloaderConfigGenerator::new("/custom/tftp/root");
        assert_eq!(gen.tftp_root, PathBuf::from("/custom/tftp/root"));
    }

    #[test]
    fn test_grub_config_with_http_boot() {
        let gen = BootloaderConfigGenerator::new("/tmp/tftp")
            .with_http_boot("http://192.168.1.100:8080");
        let content = gen.grub_config_content();
        assert!(content.contains("(http,192.168.1.100:8080)/linux"));
        assert!(content.contains("(http,192.168.1.100:8080)/initrd"));
        assert!(content.contains("via HTTP"));
    }

    #[test]
    fn test_grub_config_http_boot_with_autoinstall() {
        let config = AutoinstallConfig::new("http://192.168.1.100:8080/");
        let gen = BootloaderConfigGenerator::new("/tmp/tftp")
            .with_autoinstall(config)
            .with_http_boot("http://192.168.1.100:8080");
        let content = gen.grub_config_content();
        assert!(content.contains("(http,192.168.1.100:8080)/linux"));
        assert!(content.contains("ds=nocloud-net"));
        assert!(content.contains("(Autoinstall)"));
    }
}
