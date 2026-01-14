//! Netboot image manager.
//!
//! Downloads, verifies, and extracts netboot images for various operating systems.

use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use regex::Regex;
use sha2::{Digest, Sha256};
use tar::Archive;
use tracing::{debug, info, warn};

use super::config::NetbootConfig;

/// Manages netboot image downloads and verification.
pub struct NetbootManager {
    /// Directory to store netboot files.
    data_dir: PathBuf,
    /// Directory where extracted TFTP files are served from.
    tftp_root: PathBuf,
    /// Netboot configuration.
    config: NetbootConfig,
}

impl NetbootManager {
    /// Create a new netboot manager with the given configuration.
    ///
    /// # Arguments
    /// * `data_dir` - Directory to store downloaded files
    /// * `config` - Netboot image configuration
    pub fn new(data_dir: impl AsRef<Path>, config: NetbootConfig) -> Self {
        let data_dir = data_dir.as_ref().to_path_buf();
        let tftp_root = data_dir.join("tftp");

        Self {
            data_dir,
            tftp_root,
            config,
        }
    }

    /// Get the TFTP root directory.
    pub fn tftp_root(&self) -> &Path {
        &self.tftp_root
    }

    /// Get the netboot configuration.
    pub fn config(&self) -> &NetbootConfig {
        &self.config
    }

    /// Ensure netboot image is downloaded and ready.
    ///
    /// For Ubuntu, dynamically discovers the latest netboot filename from the releases page.
    /// Always downloads fresh since netboot images are small (<100MB).
    ///
    /// Returns the path to the TFTP root directory.
    pub fn ensure_netboot_ready(&self) -> Result<PathBuf> {
        // Create directories if needed
        fs::create_dir_all(&self.data_dir)
            .context("Failed to create data directory")?;
        fs::create_dir_all(&self.tftp_root)
            .context("Failed to create TFTP root directory")?;

        info!("Preparing {} netboot image...", self.config.name);

        // Discover actual filename for Ubuntu (may change with point releases)
        let (archive_filename, archive_url) = if self.config.id.starts_with("ubuntu") {
            self.discover_ubuntu_netboot()?
        } else {
            (self.config.archive_filename.clone(), self.config.archive_url())
        };

        let archive_path = self.data_dir.join(&archive_filename);

        // Always download fresh - netboot images are small and may be updated
        info!("Downloading {} ...", archive_url);
        self.download_archive_from_url(&archive_url, &archive_path)?;

        // Verify SHA256 if available
        if let Some(expected) = self.get_expected_sha256_for(&archive_filename)? {
            let local_sha256 = self.compute_sha256(&archive_path)?;
            if local_sha256 != expected {
                return Err(anyhow!(
                    "SHA256 verification failed. Expected: {}, Got: {}",
                    expected,
                    local_sha256
                ));
            }
            info!("SHA256 verification passed");
        }

        // Extract the archive
        self.extract_archive(&archive_path)?;

        Ok(self.tftp_root.clone())
    }

    /// Discover the latest Ubuntu netboot filename from the releases page.
    fn discover_ubuntu_netboot(&self) -> Result<(String, String)> {
        let base_url = &self.config.base_url;
        info!("Discovering netboot image from {} ...", base_url);

        let response = reqwest::blocking::get(base_url)
            .context("Failed to fetch releases page")?;

        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch releases page: HTTP {}", response.status()));
        }

        let body = response.text().context("Failed to read releases page")?;

        // Look for ubuntu-24.04.X-netboot-amd64.tar.gz pattern
        let pattern = format!(
            r#"href="(ubuntu-\d+\.\d+(?:\.\d+)?-netboot-amd64\.tar\.gz)""#
        );
        let re = Regex::new(&pattern).context("Failed to compile regex")?;

        if let Some(captures) = re.captures(&body) {
            let filename = captures.get(1).unwrap().as_str().to_string();
            let url = format!("{}/{}", base_url, filename);
            info!("Found netboot image: {}", filename);
            return Ok((filename, url));
        }

        Err(anyhow!(
            "Could not find netboot tarball on releases page. Looking for pattern: ubuntu-*.netboot-amd64.tar.gz"
        ))
    }

    /// Get expected SHA256 hash for a specific archive filename.
    fn get_expected_sha256_for(&self, archive_filename: &str) -> Result<Option<String>> {
        // First check if we have a hardcoded hash
        if let Some(ref hash) = self.config.expected_sha256 {
            return Ok(Some(hash.clone()));
        }

        // Try to fetch from SHA256SUMS
        if let Some(url) = self.config.sha256sums_url() {
            return self.fetch_sha256_from_sums(&url, archive_filename);
        }

        // No verification available
        Ok(None)
    }

    /// Fetch SHA256 hash from a SHA256SUMS file.
    fn fetch_sha256_from_sums(&self, url: &str, archive_filename: &str) -> Result<Option<String>> {
        debug!("Fetching SHA256SUMS from {}", url);

        let response = match reqwest::blocking::get(url) {
            Ok(r) => r,
            Err(e) => {
                warn!("Could not fetch SHA256SUMS: {}", e);
                return Ok(None);
            }
        };

        if !response.status().is_success() {
            warn!("SHA256SUMS not available: HTTP {}", response.status());
            return Ok(None);
        }

        let body = response.text().context("Failed to read SHA256SUMS")?;

        // Parse SHA256SUMS file to find our archive
        for line in body.lines() {
            // Format: "sha256hash *filename" or "sha256hash  filename"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let filename = parts[1].trim_start_matches('*');
                if filename == archive_filename {
                    return Ok(Some(parts[0].to_lowercase()));
                }
            }
        }

        warn!(
            "Could not find {} in SHA256SUMS",
            archive_filename
        );
        Ok(None)
    }

    /// Compute SHA256 hash of a file.
    fn compute_sha256(&self, path: &Path) -> Result<String> {
        let file = File::open(path)
            .with_context(|| format!("Failed to open {}", path.display()))?;

        let mut reader = BufReader::new(file);
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];

        loop {
            let bytes_read = reader
                .read(&mut buffer)
                .context("Failed to read file for hashing")?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let hash = hasher.finalize();
        Ok(format!("{:x}", hash))
    }

    /// Download an archive from a URL.
    fn download_archive_from_url(&self, url: &str, dest: &Path) -> Result<()> {
        let response =
            reqwest::blocking::get(url).context("Failed to start download")?;

        if !response.status().is_success() {
            return Err(anyhow!("Failed to download: HTTP {}", response.status()));
        }

        let total_size = response.content_length();
        if let Some(size) = total_size {
            info!("Download size: {:.2} MB", size as f64 / 1_048_576.0);
        }

        let mut file = File::create(dest)
            .with_context(|| format!("Failed to create {}", dest.display()))?;

        let content = response.bytes().context("Failed to download content")?;
        file.write_all(&content).context("Failed to write file")?;

        info!("Download complete: {}", dest.display());
        Ok(())
    }

    /// Extract the archive to TFTP root.
    fn extract_archive(&self, archive_path: &Path) -> Result<()> {
        info!(
            "Extracting netboot files to {} ...",
            self.tftp_root.display()
        );

        // Clear existing files
        if self.tftp_root.exists() {
            fs::remove_dir_all(&self.tftp_root).context("Failed to clear TFTP root")?;
        }
        fs::create_dir_all(&self.tftp_root).context("Failed to create TFTP root")?;

        // Determine extraction method based on file extension
        let filename = self.config.archive_filename.to_lowercase();

        if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
            self.extract_tar_gz(archive_path)?;
        } else if filename.ends_with(".tar") {
            self.extract_tar(archive_path)?;
        } else {
            // Not an archive, just copy the file directly
            self.copy_single_file(archive_path)?;
        }

        info!("Extraction complete");

        // Create symlinks for GRUB to find its files at the expected paths
        self.create_boot_symlinks()?;

        self.list_boot_files()?;

        Ok(())
    }

    /// Create symlinks so boot loaders can find files at expected paths.
    /// GRUB looks for /grub/ at TFTP root, but Ubuntu extracts to amd64/grub/
    fn create_boot_symlinks(&self) -> Result<()> {
        // Symlinks: (target_relative_path, link_name)
        // These are relative symlinks so they work regardless of absolute path
        // Ubuntu's grub.cfg references files at root level, but they're in amd64/
        let symlinks = [
            ("amd64/grub", "grub"),
            ("amd64/pxelinux.cfg", "pxelinux.cfg"),
            ("amd64/linux", "linux"),
            ("amd64/initrd", "initrd"),
        ];

        for (target, link_name) in symlinks {
            let target_path = self.tftp_root.join(target);
            let link_path = self.tftp_root.join(link_name);

            if target_path.exists() && !link_path.exists() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::symlink;
                    // Use relative path for the symlink target
                    symlink(target, &link_path).with_context(|| {
                        format!("Failed to create symlink {} -> {}", link_path.display(), target)
                    })?;
                    info!("Created symlink: {} -> {}", link_name, target);
                }
            }
        }

        Ok(())
    }

    /// Extract a .tar.gz archive.
    fn extract_tar_gz(&self, archive_path: &Path) -> Result<()> {
        let file = File::open(archive_path)
            .with_context(|| format!("Failed to open {}", archive_path.display()))?;

        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        self.extract_tar_entries(&mut archive)
    }

    /// Extract a .tar archive.
    fn extract_tar(&self, archive_path: &Path) -> Result<()> {
        let file = File::open(archive_path)
            .with_context(|| format!("Failed to open {}", archive_path.display()))?;

        let mut archive = Archive::new(file);
        self.extract_tar_entries(&mut archive)
    }

    /// Extract entries from a tar archive.
    fn extract_tar_entries<R: Read>(&self, archive: &mut Archive<R>) -> Result<()> {
        for entry in archive.entries().context("Failed to read archive")? {
            let mut entry = entry.context("Failed to read archive entry")?;
            let path = entry.path().context("Failed to get entry path")?;

            // The tarball might have a top-level directory, handle both cases
            let dest_path = if path.components().count() > 1 {
                // Skip the first component if it's a directory wrapper
                let components: Vec<_> = path.components().collect();
                let relative: PathBuf = components[1..].iter().collect();
                self.tftp_root.join(relative)
            } else {
                self.tftp_root.join(&*path)
            };

            // Create parent directories
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create directory {}", parent.display())
                })?;
            }

            // Extract the file
            if entry.header().entry_type().is_file() {
                entry
                    .unpack(&dest_path)
                    .with_context(|| format!("Failed to extract {}", dest_path.display()))?;
                debug!("Extracted: {}", dest_path.display());
            } else if entry.header().entry_type().is_dir() {
                fs::create_dir_all(&dest_path).with_context(|| {
                    format!("Failed to create directory {}", dest_path.display())
                })?;
            }
        }

        Ok(())
    }

    /// Copy a single file (for non-archive downloads like initrd.img).
    fn copy_single_file(&self, src: &Path) -> Result<()> {
        let dest = self.tftp_root.join(&self.config.archive_filename);
        fs::copy(src, &dest)
            .with_context(|| format!("Failed to copy {} to {}", src.display(), dest.display()))?;
        info!("Copied: {}", dest.display());
        Ok(())
    }

    /// List important boot files for logging.
    fn list_boot_files(&self) -> Result<()> {
        info!("Boot files available in {}:", self.tftp_root.display());

        let important_files = [
            "pxelinux.0",
            "lpxelinux.0",
            "ldlinux.c32",
            "grubnetx64.efi.signed",
            "grubx64.efi",
            "bootnetx64.efi",
            "vmlinuz",
            "initrd.img",
            "initrd",
        ];

        let mut found_any = false;
        for filename in important_files {
            let path = self.tftp_root.join(filename);
            if path.exists() {
                info!("  - {}", filename);
                found_any = true;
            }
        }

        if !found_any {
            warn!("No boot files found in root! Checking subdirectories...");
        }

        // Also check subdirectories
        self.scan_boot_directory(&self.tftp_root, 0)?;

        Ok(())
    }

    /// Recursively scan directory for boot files.
    fn scan_boot_directory(&self, dir: &Path, depth: usize) -> Result<()> {
        if depth > 3 {
            return Ok(()); // Limit recursion
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == "grub"
                        || name == "pxelinux.cfg"
                        || name == "casper"
                        || name == "EFI"
                        || name == "boot"
                    {
                        info!(
                            "  - {}/",
                            path.strip_prefix(&self.tftp_root).unwrap_or(&path).display()
                        );
                        self.scan_boot_directory(&path, depth + 1)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netboot::NetbootConfigs;

    #[test]
    fn test_new() {
        let config = NetbootConfigs::ubuntu_24_04();
        let manager = NetbootManager::new("/tmp/test-netboot", config);
        assert_eq!(manager.data_dir, PathBuf::from("/tmp/test-netboot"));
        assert_eq!(manager.tftp_root, PathBuf::from("/tmp/test-netboot/tftp"));
    }

    #[test]
    fn test_tftp_root() {
        let config = NetbootConfigs::ubuntu_24_04();
        let manager = NetbootManager::new("/var/lib/serabut", config);
        assert_eq!(manager.tftp_root(), Path::new("/var/lib/serabut/tftp"));
    }

    #[test]
    fn test_config() {
        let config = NetbootConfigs::ubuntu_24_04();
        let manager = NetbootManager::new("/tmp/test", config);
        assert_eq!(manager.config().id, "ubuntu-24.04");
    }

    #[test]
    fn test_new_with_different_os() {
        let config = NetbootConfigs::rocky_10();
        let manager = NetbootManager::new("/tmp/test-rocky", config);
        assert_eq!(manager.config().id, "rocky-10");
        assert_eq!(manager.config().name, "Rocky Linux 10");
    }

    #[test]
    fn test_new_with_alma_10() {
        let config = NetbootConfigs::alma_10();
        let manager = NetbootManager::new("/tmp/test-alma", config);
        assert_eq!(manager.config().id, "alma-10");
        assert_eq!(manager.config().name, "AlmaLinux 10");
    }

    #[test]
    fn test_new_with_debian() {
        let config = NetbootConfigs::debian_12();
        let manager = NetbootManager::new("/tmp/test-debian", config);
        assert_eq!(manager.config().id, "debian-12");
        assert!(manager.config().sha256sums_url().is_some());
    }

    #[test]
    fn test_data_dir_path() {
        let config = NetbootConfigs::ubuntu_24_04();
        let manager = NetbootManager::new("/custom/path/to/data", config);
        assert_eq!(manager.data_dir, PathBuf::from("/custom/path/to/data"));
        assert_eq!(manager.tftp_root, PathBuf::from("/custom/path/to/data/tftp"));
    }

    #[test]
    fn test_config_boot_files() {
        let config = NetbootConfigs::ubuntu_24_04();
        let manager = NetbootManager::new("/tmp/test", config);
        assert_eq!(manager.config().boot_file_bios, "amd64/pxelinux.0");
        assert_eq!(manager.config().boot_file_efi, "amd64/grubx64.efi");
    }

    #[test]
    fn test_config_boot_files_rocky() {
        let config = NetbootConfigs::rocky_10();
        let manager = NetbootManager::new("/tmp/test", config);
        assert_eq!(manager.config().boot_file_bios, "pxelinux.0");
        assert_eq!(manager.config().boot_file_efi, "grubx64.efi");
    }
}
