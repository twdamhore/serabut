//! Netboot image manager.
//!
//! Downloads, verifies, and extracts Ubuntu netboot images.

use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use tar::Archive;
use tracing::{debug, info, warn};

/// Base URL for Ubuntu releases.
const UBUNTU_RELEASES_BASE: &str = "https://releases.ubuntu.com/24.04";

/// Netboot tarball filename.
const NETBOOT_FILENAME: &str = "ubuntu-24.04.2-netboot-amd64.tar.gz";

/// SHA256SUMS filename.
const SHA256SUMS_FILENAME: &str = "SHA256SUMS";

/// Manages netboot image downloads and verification.
pub struct NetbootManager {
    /// Directory to store netboot files.
    data_dir: PathBuf,
    /// Directory where extracted TFTP files are served from.
    tftp_root: PathBuf,
}

impl NetbootManager {
    /// Create a new netboot manager.
    ///
    /// # Arguments
    /// * `data_dir` - Directory to store downloaded files
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref().to_path_buf();
        let tftp_root = data_dir.join("tftp");

        Self { data_dir, tftp_root }
    }

    /// Get the TFTP root directory.
    pub fn tftp_root(&self) -> &Path {
        &self.tftp_root
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

        info!("Checking for latest Ubuntu 24.04 netboot image...");

        // Fetch remote SHA256
        let remote_sha256 = self.fetch_remote_sha256()?;
        info!("Remote SHA256: {}", remote_sha256);

        // Check if we have the tarball and if it matches
        let tarball_path = self.data_dir.join(NETBOOT_FILENAME);
        let mut need_download = true;

        if tarball_path.exists() {
            info!("Found existing netboot tarball, verifying...");
            let local_sha256 = self.compute_sha256(&tarball_path)?;
            debug!("Local SHA256: {}", local_sha256);

            if local_sha256 == remote_sha256 {
                info!("Netboot image is up to date");
                need_download = false;
            } else {
                warn!("SHA256 mismatch, re-downloading...");
            }
        } else {
            info!("Netboot tarball not found, downloading...");
        }

        if need_download {
            // Download the tarball
            self.download_netboot(&tarball_path)?;

            // Verify the download
            let local_sha256 = self.compute_sha256(&tarball_path)?;
            if local_sha256 != remote_sha256 {
                return Err(anyhow!(
                    "SHA256 verification failed after download. Expected: {}, Got: {}",
                    remote_sha256,
                    local_sha256
                ));
            }
        }

        // Always extract to ensure files are correct
        // (in case they were modified externally)
        info!("SHA256 verification passed");

        // Extract the tarball
        self.extract_netboot(&tarball_path)?;

        Ok(self.tftp_root.clone())
    }

    /// Fetch the SHA256 checksum for the netboot tarball from Ubuntu servers.
    fn fetch_remote_sha256(&self) -> Result<String> {
        let url = format!("{}/{}", UBUNTU_RELEASES_BASE, SHA256SUMS_FILENAME);
        debug!("Fetching SHA256SUMS from {}", url);

        let response = reqwest::blocking::get(&url)
            .context("Failed to fetch SHA256SUMS")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch SHA256SUMS: HTTP {}",
                response.status()
            ));
        }

        let body = response.text().context("Failed to read SHA256SUMS")?;

        // Parse SHA256SUMS file to find our netboot tarball
        for line in body.lines() {
            // Format: "sha256hash *filename" or "sha256hash  filename"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let filename = parts[1].trim_start_matches('*');
                if filename == NETBOOT_FILENAME {
                    return Ok(parts[0].to_lowercase());
                }
            }
        }

        Err(anyhow!(
            "Could not find {} in SHA256SUMS",
            NETBOOT_FILENAME
        ))
    }

    /// Compute SHA256 hash of a file.
    fn compute_sha256(&self, path: &Path) -> Result<String> {
        let file = File::open(path)
            .with_context(|| format!("Failed to open {}", path.display()))?;

        let mut reader = BufReader::new(file);
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];

        loop {
            let bytes_read = reader.read(&mut buffer)
                .context("Failed to read file for hashing")?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let hash = hasher.finalize();
        Ok(format!("{:x}", hash))
    }

    /// Download the netboot tarball.
    fn download_netboot(&self, dest: &Path) -> Result<()> {
        let url = format!("{}/{}", UBUNTU_RELEASES_BASE, NETBOOT_FILENAME);
        info!("Downloading {} ...", url);

        let response = reqwest::blocking::get(&url)
            .context("Failed to start download")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to download: HTTP {}",
                response.status()
            ));
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

    /// Check if netboot files are already extracted.
    fn is_extracted(&self) -> bool {
        // Check for key files that should exist after extraction
        let pxelinux = self.tftp_root.join("pxelinux.0");
        let ldlinux = self.tftp_root.join("ldlinux.c32");

        pxelinux.exists() || ldlinux.exists()
    }

    /// Extract the netboot tarball to TFTP root.
    fn extract_netboot(&self, tarball: &Path) -> Result<()> {
        info!("Extracting netboot files to {} ...", self.tftp_root.display());

        // Clear existing files
        if self.tftp_root.exists() {
            fs::remove_dir_all(&self.tftp_root)
                .context("Failed to clear TFTP root")?;
        }
        fs::create_dir_all(&self.tftp_root)
            .context("Failed to create TFTP root")?;

        let file = File::open(tarball)
            .with_context(|| format!("Failed to open {}", tarball.display()))?;

        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        // Extract all files
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
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }

            // Extract the file
            if entry.header().entry_type().is_file() {
                entry.unpack(&dest_path)
                    .with_context(|| format!("Failed to extract {}", dest_path.display()))?;
                debug!("Extracted: {}", dest_path.display());
            } else if entry.header().entry_type().is_dir() {
                fs::create_dir_all(&dest_path)
                    .with_context(|| format!("Failed to create directory {}", dest_path.display()))?;
            }
        }

        info!("Extraction complete");

        // List key files
        self.list_boot_files()?;

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
                    let name = path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    if name == "grub" || name == "pxelinux.cfg" || name == "casper" {
                        info!("  - {}/", path.strip_prefix(&self.tftp_root).unwrap_or(&path).display());
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
    use std::env;

    #[test]
    fn test_new() {
        let manager = NetbootManager::new("/tmp/test-netboot");
        assert_eq!(manager.data_dir, PathBuf::from("/tmp/test-netboot"));
        assert_eq!(manager.tftp_root, PathBuf::from("/tmp/test-netboot/tftp"));
    }

    #[test]
    fn test_tftp_root() {
        let manager = NetbootManager::new("/var/lib/serabut");
        assert_eq!(manager.tftp_root(), Path::new("/var/lib/serabut/tftp"));
    }
}
