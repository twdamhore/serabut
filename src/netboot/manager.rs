//! Netboot image manager.
//!
//! Downloads, verifies, and extracts netboot images for various operating systems.

use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
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

    /// Ensure netboot image is downloaded and up to date.
    ///
    /// Returns the path to the TFTP root directory.
    pub fn ensure_netboot_ready(&self) -> Result<PathBuf> {
        // Create directories if needed
        fs::create_dir_all(&self.data_dir)
            .context("Failed to create data directory")?;
        fs::create_dir_all(&self.tftp_root)
            .context("Failed to create TFTP root directory")?;

        info!("Checking for {} netboot image...", self.config.name);

        let archive_path = self.data_dir.join(&self.config.archive_filename);
        let mut need_download = true;

        // Try to verify existing file
        if archive_path.exists() {
            info!("Found existing archive, verifying...");

            if let Some(expected) = self.get_expected_sha256()? {
                let local_sha256 = self.compute_sha256(&archive_path)?;
                debug!("Local SHA256: {}", local_sha256);
                debug!("Expected SHA256: {}", expected);

                if local_sha256 == expected {
                    info!("Archive is up to date");
                    need_download = false;
                } else {
                    warn!("SHA256 mismatch, re-downloading...");
                }
            } else {
                // No SHA256 verification available, check if file is non-empty
                let metadata = fs::metadata(&archive_path)?;
                if metadata.len() > 0 {
                    info!("Archive exists (no SHA256 verification available)");
                    need_download = false;
                }
            }
        } else {
            info!("Archive not found, downloading...");
        }

        if need_download {
            self.download_archive(&archive_path)?;

            // Verify if possible
            if let Some(expected) = self.get_expected_sha256()? {
                let local_sha256 = self.compute_sha256(&archive_path)?;
                if local_sha256 != expected {
                    return Err(anyhow!(
                        "SHA256 verification failed after download. Expected: {}, Got: {}",
                        expected,
                        local_sha256
                    ));
                }
                info!("SHA256 verification passed");
            }
        }

        // Always extract to ensure files are correct
        // (in case they were modified externally)
        self.extract_archive(&archive_path)?;

        Ok(self.tftp_root.clone())
    }

    /// Get expected SHA256 hash for the archive.
    fn get_expected_sha256(&self) -> Result<Option<String>> {
        // First check if we have a hardcoded hash
        if let Some(ref hash) = self.config.expected_sha256 {
            return Ok(Some(hash.clone()));
        }

        // Try to fetch from SHA256SUMS
        if let Some(url) = self.config.sha256sums_url() {
            return self.fetch_sha256_from_sums(&url);
        }

        // No verification available
        Ok(None)
    }

    /// Fetch SHA256 hash from a SHA256SUMS file.
    fn fetch_sha256_from_sums(&self, url: &str) -> Result<Option<String>> {
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
                if filename == self.config.archive_filename {
                    return Ok(Some(parts[0].to_lowercase()));
                }
            }
        }

        warn!(
            "Could not find {} in SHA256SUMS",
            self.config.archive_filename
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

    /// Download the archive.
    fn download_archive(&self, dest: &Path) -> Result<()> {
        let url = self.config.archive_url();
        info!("Downloading {} ...", url);

        let response =
            reqwest::blocking::get(&url).context("Failed to start download")?;

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
        self.list_boot_files()?;

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
        info!("Boot files available:");

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

        for filename in important_files {
            let path = self.tftp_root.join(filename);
            if path.exists() {
                info!("  - {}", filename);
            }
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
}
