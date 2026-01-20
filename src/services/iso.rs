//! ISO service for managing ISO files and reading their contents.
//!
//! Handles iso.cfg parsing, ISO9660 reading, and template detection.

use crate::error::{AppError, AppResult};
use bytes::Bytes;
use gpt_disk_io::BlockIo;
use gpt_disk_types::{BlockSize, Lba};
use iso9660::{find_file, mount};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;
use tokio::sync::mpsc;

const ISO_BLOCK_SIZE: u64 = 2048;
/// Chunk size for streaming (8MB).
const CHUNK_SIZE: usize = 8 * 1024 * 1024;
/// Channel capacity for streaming. With 8MB chunks, this allows up to 16MB in flight.
const CHANNEL_CAPACITY: usize = 2;

/// Stream file contents in chunks to a channel.
///
/// Reads the file in CHUNK_SIZE chunks and sends each chunk to the channel.
/// Stops early if receiver is dropped.
fn stream_file_to_channel(
    file: &mut File,
    file_size: u64,
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
) -> Result<(), std::io::Error> {
    let mut bytes_remaining = file_size as usize;

    while bytes_remaining > 0 {
        let chunk_size = std::cmp::min(bytes_remaining, CHUNK_SIZE);

        let mut buffer = vec![0u8; chunk_size];
        file.read_exact(&mut buffer)?;

        let bytes = Bytes::from(buffer);
        if tx.blocking_send(Ok(bytes)).is_err() {
            // Receiver dropped, stop sending
            return Ok(());
        }

        bytes_remaining -= chunk_size;
    }

    Ok(())
}

/// Wrapper to implement BlockIo for std::fs::File.
struct FileBlockIo {
    file: File,
    num_blocks: u64,
}

impl FileBlockIo {
    fn new(mut file: File) -> std::io::Result<Self> {
        let size = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;
        let num_blocks = size / ISO_BLOCK_SIZE;
        Ok(Self { file, num_blocks })
    }
}

impl BlockIo for FileBlockIo {
    type Error = std::io::Error;

    fn block_size(&self) -> BlockSize {
        BlockSize::from_usize(ISO_BLOCK_SIZE as usize).unwrap()
    }

    fn num_blocks(&mut self) -> Result<u64, Self::Error> {
        Ok(self.num_blocks)
    }

    fn read_blocks(
        &mut self,
        start_lba: Lba,
        dst: &mut [u8],
    ) -> Result<(), Self::Error> {
        let offset = start_lba.0 * ISO_BLOCK_SIZE;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(dst)?;
        Ok(())
    }

    fn write_blocks(
        &mut self,
        _start_lba: Lba,
        _src: &[u8],
    ) -> Result<(), Self::Error> {
        // Read-only, no writes needed
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// ISO configuration from iso.cfg.
#[derive(Debug, Clone)]
pub struct IsoConfig {
    pub filename: String,
    /// Path to initrd inside the ISO (for firmware concatenation).
    pub initrd_path: Option<String>,
    /// Firmware file to append to initrd (e.g., firmware.cpio.gz).
    pub firmware: Option<String>,
}

/// Service for reading ISO files and their contents.
pub struct IsoService {
    config_path: PathBuf,
}

impl IsoService {
    /// Create a new ISO service.
    pub fn new(config_path: PathBuf) -> Self {
        Self { config_path }
    }

    /// Validate ISO directory structure at startup and log warnings for issues.
    pub fn validate_startup(&self) {
        let iso_dir = self.config_path.join("iso");

        if !iso_dir.exists() {
            tracing::warn!(
                "ISO directory does not exist: {:?}. \
                Create this directory and add ISO subdirectories (e.g., ubuntu-24.04.3/) \
                to enable PXE boot functionality.",
                iso_dir
            );
            return;
        }

        let subdirs: Vec<_> = match std::fs::read_dir(&iso_dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .collect(),
            Err(e) => {
                tracing::warn!(
                    "Cannot read ISO directory {:?}: {}. Check directory permissions.",
                    iso_dir,
                    e
                );
                return;
            }
        };

        if subdirs.is_empty() {
            tracing::warn!(
                "ISO directory is empty: {:?}. \
                Create subdirectories for each OS (e.g., ubuntu-24.04.3/, alma-9.4/) \
                containing iso.cfg and the ISO file.",
                iso_dir
            );
            return;
        }

        for entry in subdirs {
            let iso_name = entry.file_name();
            let iso_name_str = iso_name.to_string_lossy();
            self.validate_iso_subdir(&iso_name_str, &entry.path());
        }
    }

    fn validate_iso_subdir(&self, iso_name: &str, iso_path: &std::path::Path) {
        let iso_cfg_path = iso_path.join("iso.cfg");

        if !iso_cfg_path.exists() {
            tracing::warn!(
                "ISO '{}': missing iso.cfg at {:?}. \
                Create this file with 'filename=<iso-file-name>' to specify the ISO file.",
                iso_name,
                iso_cfg_path
            );
            return;
        }

        let content = match std::fs::read_to_string(&iso_cfg_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "ISO '{}': cannot read iso.cfg at {:?}: {}. Check file permissions.",
                    iso_name,
                    iso_cfg_path,
                    e
                );
                return;
            }
        };

        let filename = content
            .lines()
            .filter_map(|line| parse_config_line(line))
            .find(|(key, _)| *key == "filename")
            .map(|(_, value)| value.to_string());

        let filename = match filename {
            Some(f) if !f.is_empty() => f,
            _ => {
                tracing::warn!(
                    "ISO '{}': iso.cfg at {:?} is missing 'filename=' entry. \
                    Add 'filename=<iso-file-name>' to specify the ISO file.",
                    iso_name,
                    iso_cfg_path
                );
                return;
            }
        };

        let iso_file_path = iso_path.join(&filename);
        if !iso_file_path.exists() {
            tracing::warn!(
                "ISO '{}': ISO file does not exist: {:?}. \
                Download or copy the ISO file to this location.",
                iso_name,
                iso_file_path
            );
            return;
        }

        if let Err(e) = File::open(&iso_file_path) {
            tracing::warn!(
                "ISO '{}': ISO file exists but cannot be read: {:?}: {}. \
                Check file permissions.",
                iso_name,
                iso_file_path,
                e
            );
            return;
        }

        let boot_template = iso_path.join("boot.ipxe.j2");
        if !boot_template.exists() {
            tracing::warn!(
                "ISO '{}': missing boot.ipxe.j2 at {:?}. \
                See https://github.com/twdamhore/serabut#directory-structure for template examples.",
                iso_name,
                boot_template
            );
        }

        let automation_dir = iso_path.join("automation");
        if !automation_dir.exists() {
            tracing::warn!(
                "ISO '{}': missing automation/ directory at {:?}. \
                Create automation profiles (e.g., automation/default/) with user-data.j2 or kickstart.ks.j2. \
                See https://github.com/twdamhore/serabut#directory-structure",
                iso_name,
                automation_dir
            );
        } else {
            let profiles: Vec<_> = std::fs::read_dir(&automation_dir)
                .ok()
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().is_dir())
                        .collect()
                })
                .unwrap_or_default();

            if profiles.is_empty() {
                tracing::warn!(
                    "ISO '{}': automation/ directory is empty. \
                    Create profile subdirectories (e.g., automation/default/) with templates. \
                    See https://github.com/twdamhore/serabut#directory-structure",
                    iso_name
                );
            } else {
                for profile in &profiles {
                    let profile_name = profile.file_name();
                    tracing::info!(
                        "ISO '{}': found automation profile '{}'",
                        iso_name,
                        profile_name.to_string_lossy()
                    );
                }
            }
        }

        tracing::info!("ISO '{}': validated successfully ({})", iso_name, filename);
    }

    fn iso_dir(&self, iso_name: &str) -> PathBuf {
        self.config_path.join("iso").join(iso_name)
    }

    fn iso_cfg_path(&self, iso_name: &str) -> PathBuf {
        self.iso_dir(iso_name).join("iso.cfg")
    }

    /// Load ISO configuration.
    pub fn load_config(&self, iso_name: &str) -> AppResult<IsoConfig> {
        let path = self.iso_cfg_path(iso_name);

        if !path.exists() {
            return Err(AppError::IsoConfigNotFound { path });
        }

        let file = File::open(&path).map_err(|e| AppError::FileRead {
            path: path.clone(),
            source: e,
        })?;

        let reader = BufReader::new(file);
        let mut filename = None;
        let mut initrd_path = None;
        let mut firmware = None;

        for line in reader.lines() {
            let line = line.map_err(|e| AppError::FileRead {
                path: path.clone(),
                source: e,
            })?;

            if let Some((key, value)) = parse_config_line(&line) {
                match key {
                    "filename" => filename = Some(value.to_string()),
                    "initrd_path" => initrd_path = Some(value.to_string()),
                    "firmware" => firmware = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        let filename = filename.ok_or_else(|| AppError::ConfigParse {
            path: path.clone(),
            message: "Missing required 'filename' field".to_string(),
        })?;

        Ok(IsoConfig {
            filename,
            initrd_path,
            firmware,
        })
    }

    /// Get the full path to the ISO file.
    pub fn iso_file_path(&self, iso_name: &str) -> AppResult<PathBuf> {
        let config = self.load_config(iso_name)?;
        let path = self.iso_dir(iso_name).join(&config.filename);

        if !path.exists() {
            return Err(AppError::IsoFileNotFound { path });
        }

        Ok(path)
    }

    /// Check if a path is the ISO file itself.
    pub fn is_iso_file(&self, iso_name: &str, path: &str) -> AppResult<bool> {
        let config = self.load_config(iso_name)?;
        Ok(path == config.filename)
    }

    /// Check if a template exists for the given path.
    ///
    /// Handles paths with MAC addresses: automation/{profile}/{mac}/{file}
    /// will look for template at automation/{profile}/{file}.j2
    pub fn template_path(&self, iso_name: &str, path: &str) -> Option<PathBuf> {
        // First try direct path
        let template_path = self.iso_dir(iso_name).join(format!("{}.j2", path));
        if template_path.exists() {
            return Some(template_path);
        }

        // Check if path matches automation/{profile}/{mac}/{file}
        // If so, try automation/{profile}/{file}.j2
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 4 && parts[0] == "automation" {
            // parts[0] = "automation"
            // parts[1] = profile
            // parts[2] = mac (skip this)
            // parts[3..] = file path
            let template_path_without_mac =
                format!("automation/{}/{}", parts[1], parts[3..].join("/"));
            let template_path = self
                .iso_dir(iso_name)
                .join(format!("{}.j2", template_path_without_mac));
            if template_path.exists() {
                return Some(template_path);
            }
        }

        None
    }

    /// Check if firmware concatenation is configured and path matches initrd_path.
    ///
    /// Returns Some((initrd_path, firmware)) if the requested path matches initrd_path
    /// and firmware is configured. Returns None otherwise.
    pub fn should_concat_firmware(&self, iso_name: &str, path: &str) -> AppResult<Option<(String, String)>> {
        let config = self.load_config(iso_name)?;

        // Normalize path for comparison (handle leading slash variations)
        let normalized_path = path.trim_start_matches('/');

        if let (Some(initrd_path), Some(firmware)) = (config.initrd_path, config.firmware) {
            let normalized_initrd = initrd_path.trim_start_matches('/');
            if normalized_path == normalized_initrd {
                return Ok(Some((initrd_path, firmware)));
            }
        }

        Ok(None)
    }

    /// Get the boot template path for an ISO.
    ///
    /// Checks automation profile first, then falls back to ISO-level template.
    /// Order: iso/{iso}/automation/{profile}/boot.ipxe.j2 -> iso/{iso}/boot.ipxe.j2
    pub fn boot_template_path(&self, iso_name: &str, automation: Option<&str>) -> AppResult<PathBuf> {
        // Check automation profile specific template first
        if let Some(profile) = automation {
            let profile_path = self
                .iso_dir(iso_name)
                .join("automation")
                .join(profile)
                .join("boot.ipxe.j2");
            if profile_path.exists() {
                tracing::info!(
                    "Using profile-specific boot template: {:?}",
                    profile_path
                );
                return Ok(profile_path);
            }
        }

        // Fall back to ISO-level template
        let iso_path = self.iso_dir(iso_name).join("boot.ipxe.j2");
        if iso_path.exists() {
            tracing::info!("Using ISO-level boot template: {:?}", iso_path);
            return Ok(iso_path);
        }

        Err(AppError::TemplateNotFound { path: iso_path })
    }

    /// Stream the ISO file itself with chunked reads for memory efficiency.
    ///
    /// Returns the file size and a receiver that yields chunks.
    /// Uses spawn_blocking for the file reads with backpressure via bounded channel.
    pub fn stream_iso_file(
        &self,
        iso_name: &str,
    ) -> AppResult<(u64, mpsc::Receiver<Result<Bytes, std::io::Error>>)> {
        let iso_path = self.iso_file_path(iso_name)?;

        // Get file size
        let metadata = std::fs::metadata(&iso_path).map_err(|e| AppError::FileRead {
            path: iso_path.clone(),
            source: e,
        })?;
        let file_size = metadata.len();

        // Create bounded channel for backpressure
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);

        // Spawn blocking task to read chunks
        tokio::task::spawn_blocking(move || {
            let result = (|| -> Result<(), std::io::Error> {
                let mut file = File::open(&iso_path)?;
                stream_file_to_channel(&mut file, file_size, &tx)?;
                Ok(())
            })();

            if let Err(e) = result {
                let _ = tx.blocking_send(Err(e));
            }
        });

        Ok((file_size, rx))
    }

    /// Stream a file from within an ISO.
    ///
    /// Returns the file size and a receiver that yields chunks.
    /// Uses spawn_blocking for the synchronous ISO reads.
    pub fn stream_from_iso(
        &self,
        iso_name: &str,
        file_path: &str,
    ) -> AppResult<(u64, mpsc::Receiver<Result<Bytes, std::io::Error>>)> {
        let iso_path = self.iso_file_path(iso_name)?;

        // Open ISO and find file entry to get size
        let file = File::open(&iso_path).map_err(|e| AppError::FileRead {
            path: iso_path.clone(),
            source: e,
        })?;

        let mut block_io = FileBlockIo::new(file).map_err(|e| AppError::FileRead {
            path: iso_path.clone(),
            source: e,
        })?;

        let volume = mount(&mut block_io, 0).map_err(|e| AppError::IsoRead {
            path: iso_path.clone(),
            message: format!("Failed to mount ISO: {}", e),
        })?;

        // Normalize path - ensure leading slash
        let normalized_path = if file_path.starts_with('/') {
            file_path.to_string()
        } else {
            format!("/{}", file_path)
        };

        tracing::debug!("Looking for file in ISO: {}", normalized_path);

        let entry = find_file(&mut block_io, &volume, &normalized_path).map_err(|e| {
            tracing::debug!("File not found in ISO: {}", e);
            AppError::FileNotFoundInIso {
                iso: iso_name.to_string(),
                path: file_path.to_string(),
            }
        })?;

        let file_size = entry.size;
        let extent_lba = entry.extent_lba;

        // Create bounded channel for backpressure
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);

        let iso_path_clone = iso_path.clone();

        // Spawn blocking task to read chunks.
        // We re-open the ISO here because FileBlockIo contains a File handle
        // which is not Send and cannot be moved into the spawned task.
        tokio::task::spawn_blocking(move || {
            let result = (|| -> Result<(), std::io::Error> {
                let file = File::open(&iso_path_clone)?;
                let mut block_io = FileBlockIo::new(file)?;

                let mut offset: u64 = 0;
                let total_size = file_size;

                while offset < total_size {
                    let remaining = total_size - offset;
                    let chunk_size = std::cmp::min(remaining as usize, CHUNK_SIZE);

                    // Calculate sector-aligned read
                    let start_lba = extent_lba as u64 + (offset / ISO_BLOCK_SIZE);
                    let sectors_needed = (chunk_size as u64).div_ceil(ISO_BLOCK_SIZE);
                    let read_size = (sectors_needed * ISO_BLOCK_SIZE) as usize;

                    let mut buffer = vec![0u8; read_size];
                    block_io.read_blocks(Lba(start_lba), &mut buffer)?;

                    // Truncate to actual chunk size (handle last partial chunk)
                    buffer.truncate(chunk_size);

                    let bytes = Bytes::from(buffer);
                    if tx.blocking_send(Ok(bytes)).is_err() {
                        // Receiver dropped, stop sending
                        break;
                    }

                    offset += chunk_size as u64;
                }

                Ok(())
            })();

            if let Err(e) = result {
                let _ = tx.blocking_send(Err(e));
            }
        });

        Ok((file_size, rx))
    }

    /// Stream initrd from ISO with firmware file concatenated.
    ///
    /// Returns the combined size and a receiver that yields chunks.
    /// First streams all initrd chunks, then firmware chunks.
    pub fn stream_initrd_with_firmware(
        &self,
        iso_name: &str,
        initrd_path: &str,
        firmware_filename: &str,
    ) -> AppResult<(u64, mpsc::Receiver<Result<Bytes, std::io::Error>>)> {
        let iso_path = self.iso_file_path(iso_name)?;
        let firmware_path = self.iso_dir(iso_name).join(firmware_filename);

        // Get initrd file entry for size
        let file = File::open(&iso_path).map_err(|e| AppError::FileRead {
            path: iso_path.clone(),
            source: e,
        })?;

        let mut block_io = FileBlockIo::new(file).map_err(|e| AppError::FileRead {
            path: iso_path.clone(),
            source: e,
        })?;

        let volume = mount(&mut block_io, 0).map_err(|e| AppError::IsoRead {
            path: iso_path.clone(),
            message: format!("Failed to mount ISO: {}", e),
        })?;

        // Normalize path - ensure leading slash
        let normalized_path = if initrd_path.starts_with('/') {
            initrd_path.to_string()
        } else {
            format!("/{}", initrd_path)
        };

        tracing::debug!("Looking for initrd in ISO: {}", normalized_path);

        let entry = find_file(&mut block_io, &volume, &normalized_path).map_err(|e| {
            tracing::debug!("Initrd not found in ISO: {}", e);
            AppError::FileNotFoundInIso {
                iso: iso_name.to_string(),
                path: initrd_path.to_string(),
            }
        })?;

        let initrd_size = entry.size;
        let extent_lba = entry.extent_lba;

        // Get firmware size
        let firmware_metadata = std::fs::metadata(&firmware_path).map_err(|e| AppError::FileRead {
            path: firmware_path.clone(),
            source: e,
        })?;
        let firmware_size = firmware_metadata.len();

        let total_size = initrd_size + firmware_size;

        tracing::info!(
            "Streaming initrd ({} bytes) + firmware ({} bytes) = {} bytes total",
            initrd_size,
            firmware_size,
            total_size
        );

        // Create bounded channel for backpressure
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);

        let iso_path_clone = iso_path.clone();
        let firmware_path_clone = firmware_path.clone();

        // Spawn blocking task to read chunks.
        // We re-open the ISO here because FileBlockIo contains a File handle
        // which is not Send and cannot be moved into the spawned task.
        tokio::task::spawn_blocking(move || {
            let result = (|| -> Result<(), std::io::Error> {
                // Phase 1: Stream initrd from ISO
                let file = File::open(&iso_path_clone)?;
                let mut block_io = FileBlockIo::new(file)?;

                let mut offset: u64 = 0;
                while offset < initrd_size {
                    let remaining = initrd_size - offset;
                    let chunk_size = std::cmp::min(remaining as usize, CHUNK_SIZE);

                    let start_lba = extent_lba as u64 + (offset / ISO_BLOCK_SIZE);
                    let sectors_needed = (chunk_size as u64).div_ceil(ISO_BLOCK_SIZE);
                    let read_size = (sectors_needed * ISO_BLOCK_SIZE) as usize;

                    let mut buffer = vec![0u8; read_size];
                    block_io.read_blocks(Lba(start_lba), &mut buffer)?;
                    buffer.truncate(chunk_size);

                    let bytes = Bytes::from(buffer);
                    if tx.blocking_send(Ok(bytes)).is_err() {
                        return Ok(());
                    }

                    offset += chunk_size as u64;
                }

                // Phase 2: Stream firmware from disk
                let mut firmware_file = File::open(&firmware_path_clone)?;
                stream_file_to_channel(&mut firmware_file, firmware_size, &tx)?;

                Ok(())
            })();

            if let Err(e) = result {
                let _ = tx.blocking_send(Err(e));
            }
        });

        Ok((total_size, rx))
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
    fn test_load_iso_config() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        std::fs::create_dir_all(&iso_dir).unwrap();
        std::fs::write(
            iso_dir.join("iso.cfg"),
            "filename=ubuntu-24.04-live-server.iso\n",
        )
        .unwrap();

        let service = IsoService::new(dir.path().to_path_buf());
        let config = service.load_config("ubuntu-24.04").unwrap();

        assert_eq!(config.filename, "ubuntu-24.04-live-server.iso");
    }

    #[test]
    fn test_load_iso_config_not_found() {
        let dir = setup_test_dir();
        let service = IsoService::new(dir.path().to_path_buf());

        let result = service.load_config("nonexistent");
        assert!(matches!(result, Err(AppError::IsoConfigNotFound { .. })));
    }

    #[test]
    fn test_is_iso_file() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        std::fs::create_dir_all(&iso_dir).unwrap();
        std::fs::write(iso_dir.join("iso.cfg"), "filename=ubuntu.iso\n").unwrap();

        let service = IsoService::new(dir.path().to_path_buf());

        assert!(service.is_iso_file("ubuntu-24.04", "ubuntu.iso").unwrap());
        assert!(!service.is_iso_file("ubuntu-24.04", "other.iso").unwrap());
    }

    #[test]
    fn test_template_path() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        let auto_dir = iso_dir.join("automation").join("minimal");
        std::fs::create_dir_all(&auto_dir).unwrap();
        std::fs::write(auto_dir.join("user-data.j2"), "template content").unwrap();

        let service = IsoService::new(dir.path().to_path_buf());

        let template = service.template_path("ubuntu-24.04", "automation/minimal/user-data");
        assert!(template.is_some());

        let no_template = service.template_path("ubuntu-24.04", "automation/minimal/meta-data");
        assert!(no_template.is_none());
    }

    #[test]
    fn test_template_path_with_mac_in_path() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        let auto_dir = iso_dir.join("automation").join("default");
        std::fs::create_dir_all(&auto_dir).unwrap();
        std::fs::write(auto_dir.join("user-data.j2"), "template content").unwrap();
        std::fs::write(auto_dir.join("meta-data.j2"), "meta content").unwrap();

        let service = IsoService::new(dir.path().to_path_buf());

        // Path with MAC should find template without MAC
        let template =
            service.template_path("ubuntu-24.04", "automation/default/aa-bb-cc-dd-ee-ff/user-data");
        assert!(template.is_some());
        assert!(template.unwrap().ends_with("automation/default/user-data.j2"));

        let template =
            service.template_path("ubuntu-24.04", "automation/default/aa-bb-cc-dd-ee-ff/meta-data");
        assert!(template.is_some());
        assert!(template.unwrap().ends_with("automation/default/meta-data.j2"));
    }

    #[test]
    fn test_boot_template_path_iso_level() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        std::fs::create_dir_all(&iso_dir).unwrap();
        std::fs::write(iso_dir.join("boot.ipxe.j2"), "boot template").unwrap();

        let service = IsoService::new(dir.path().to_path_buf());
        let path = service.boot_template_path("ubuntu-24.04", None).unwrap();

        assert!(path.exists());
        assert!(path.ends_with("boot.ipxe.j2"));
    }

    #[test]
    fn test_boot_template_path_profile_override() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        let profile_dir = iso_dir.join("automation").join("docker");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(iso_dir.join("boot.ipxe.j2"), "iso template").unwrap();
        std::fs::write(profile_dir.join("boot.ipxe.j2"), "profile template").unwrap();

        let service = IsoService::new(dir.path().to_path_buf());

        // With profile, should use profile-specific
        let path = service.boot_template_path("ubuntu-24.04", Some("docker")).unwrap();
        assert!(path.to_string_lossy().contains("automation/docker"));

        // Without profile, should use ISO-level
        let path = service.boot_template_path("ubuntu-24.04", None).unwrap();
        assert!(!path.to_string_lossy().contains("automation"));
    }

    #[test]
    fn test_boot_template_path_not_found() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        std::fs::create_dir_all(&iso_dir).unwrap();

        let service = IsoService::new(dir.path().to_path_buf());
        let result = service.boot_template_path("ubuntu-24.04", None);

        assert!(matches!(result, Err(AppError::TemplateNotFound { .. })));
    }

    #[test]
    fn test_load_iso_config_with_firmware() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("debian-13");
        std::fs::create_dir_all(&iso_dir).unwrap();
        std::fs::write(
            iso_dir.join("iso.cfg"),
            "filename=debian-13.3.0-amd64-netinst.iso\ninitrd_path=/install.amd/initrd.gz\nfirmware=firmware.cpio.gz\n",
        )
        .unwrap();

        let service = IsoService::new(dir.path().to_path_buf());
        let config = service.load_config("debian-13").unwrap();

        assert_eq!(config.filename, "debian-13.3.0-amd64-netinst.iso");
        assert_eq!(config.initrd_path, Some("/install.amd/initrd.gz".to_string()));
        assert_eq!(config.firmware, Some("firmware.cpio.gz".to_string()));
    }

    #[test]
    fn test_load_iso_config_without_firmware() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        std::fs::create_dir_all(&iso_dir).unwrap();
        std::fs::write(
            iso_dir.join("iso.cfg"),
            "filename=ubuntu-24.04-live-server.iso\n",
        )
        .unwrap();

        let service = IsoService::new(dir.path().to_path_buf());
        let config = service.load_config("ubuntu-24.04").unwrap();

        assert_eq!(config.filename, "ubuntu-24.04-live-server.iso");
        assert_eq!(config.initrd_path, None);
        assert_eq!(config.firmware, None);
    }

    #[test]
    fn test_should_concat_firmware_matches() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("debian-13");
        std::fs::create_dir_all(&iso_dir).unwrap();
        std::fs::write(
            iso_dir.join("iso.cfg"),
            "filename=debian.iso\ninitrd_path=/install.amd/initrd.gz\nfirmware=firmware.cpio.gz\n",
        )
        .unwrap();

        let service = IsoService::new(dir.path().to_path_buf());

        // Should match with leading slash
        let result = service.should_concat_firmware("debian-13", "/install.amd/initrd.gz").unwrap();
        assert!(result.is_some());
        let (initrd, fw) = result.unwrap();
        assert_eq!(initrd, "/install.amd/initrd.gz");
        assert_eq!(fw, "firmware.cpio.gz");

        // Should match without leading slash
        let result = service.should_concat_firmware("debian-13", "install.amd/initrd.gz").unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_should_concat_firmware_no_match() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("debian-13");
        std::fs::create_dir_all(&iso_dir).unwrap();
        std::fs::write(
            iso_dir.join("iso.cfg"),
            "filename=debian.iso\ninitrd_path=/install.amd/initrd.gz\nfirmware=firmware.cpio.gz\n",
        )
        .unwrap();

        let service = IsoService::new(dir.path().to_path_buf());

        // Different path should not match
        let result = service.should_concat_firmware("debian-13", "/install.amd/vmlinuz").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_should_concat_firmware_not_configured() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        std::fs::create_dir_all(&iso_dir).unwrap();
        std::fs::write(
            iso_dir.join("iso.cfg"),
            "filename=ubuntu.iso\n",
        )
        .unwrap();

        let service = IsoService::new(dir.path().to_path_buf());

        // No firmware configured, should return None
        let result = service.should_concat_firmware("ubuntu-24.04", "/casper/initrd").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_should_concat_firmware_partial_config() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("test");
        std::fs::create_dir_all(&iso_dir).unwrap();

        // Only initrd_path, no firmware
        std::fs::write(
            iso_dir.join("iso.cfg"),
            "filename=test.iso\ninitrd_path=/install/initrd.gz\n",
        )
        .unwrap();

        let service = IsoService::new(dir.path().to_path_buf());
        let result = service.should_concat_firmware("test", "/install/initrd.gz").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_stream_file_to_channel() {
        // Create a test file with known content
        let dir = setup_test_dir();
        let test_file = dir.path().join("test.bin");
        let test_data = vec![0xABu8; 1024 * 100]; // 100KB of 0xAB
        std::fs::write(&test_file, &test_data).unwrap();

        // Create channel and stream
        let (tx, mut rx) = mpsc::channel(CHANNEL_CAPACITY);
        let mut file = File::open(&test_file).unwrap();
        let file_size = test_data.len() as u64;

        // Run in a thread since blocking_send requires it
        std::thread::spawn(move || {
            stream_file_to_channel(&mut file, file_size, &tx).unwrap();
        });

        // Collect all chunks
        let mut received = Vec::new();
        while let Some(result) = rx.blocking_recv() {
            let bytes = result.unwrap();
            received.extend_from_slice(&bytes);
        }

        assert_eq!(received.len(), test_data.len());
        assert_eq!(received, test_data);
    }

    #[test]
    fn test_stream_file_to_channel_multiple_chunks() {
        // Create a file larger than CHUNK_SIZE (8MB) to test chunking
        // 20MB = 2 full chunks (8MB each) + 1 partial chunk (4MB)
        let dir = setup_test_dir();
        let test_file = dir.path().join("large.bin");
        let file_size = 20 * 1024 * 1024; // 20MB
        let test_data: Vec<u8> = (0..file_size).map(|i| (i % 256) as u8).collect();
        std::fs::write(&test_file, &test_data).unwrap();

        let (tx, mut rx) = mpsc::channel(CHANNEL_CAPACITY);
        let mut file = File::open(&test_file).unwrap();

        std::thread::spawn(move || {
            stream_file_to_channel(&mut file, file_size as u64, &tx).unwrap();
        });

        // Collect chunks and verify we get multiple
        let mut received = Vec::new();
        let mut chunk_count = 0;
        while let Some(result) = rx.blocking_recv() {
            let bytes = result.unwrap();
            chunk_count += 1;
            received.extend_from_slice(&bytes);
        }

        assert_eq!(chunk_count, 3); // 8MB + 8MB + 4MB
        assert_eq!(received.len(), test_data.len());
        assert_eq!(received, test_data);
    }

    #[tokio::test]
    async fn test_stream_iso_file() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("test-iso");
        std::fs::create_dir_all(&iso_dir).unwrap();

        // Create iso.cfg
        std::fs::write(iso_dir.join("iso.cfg"), "filename=test.iso\n").unwrap();

        // Create a test "ISO" file with known content (1MB)
        let test_data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
        std::fs::write(iso_dir.join("test.iso"), &test_data).unwrap();

        let service = IsoService::new(dir.path().to_path_buf());
        let (size, mut rx) = service.stream_iso_file("test-iso").unwrap();

        assert_eq!(size, test_data.len() as u64);

        // Collect all chunks
        let mut received = Vec::new();
        while let Some(result) = rx.recv().await {
            let bytes = result.unwrap();
            received.extend_from_slice(&bytes);
        }

        assert_eq!(received.len(), test_data.len());
        assert_eq!(received, test_data);
    }

    #[tokio::test]
    async fn test_stream_iso_file_not_found() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("test-iso");
        std::fs::create_dir_all(&iso_dir).unwrap();

        // Create iso.cfg pointing to non-existent file
        std::fs::write(iso_dir.join("iso.cfg"), "filename=missing.iso\n").unwrap();

        let service = IsoService::new(dir.path().to_path_buf());
        let result = service.stream_iso_file("test-iso");

        assert!(matches!(result, Err(AppError::IsoFileNotFound { .. })));
    }
}
