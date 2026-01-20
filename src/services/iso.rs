//! ISO service for managing ISO files and reading their contents.
//!
//! Handles iso.cfg parsing, ISO9660 reading, tar.gz extraction, and template detection.

use crate::error::{AppError, AppResult};
use flate2::read::GzDecoder;
use gpt_disk_io::BlockIo;
use gpt_disk_types::{BlockSize, Lba};
use iso9660::{find_file, mount, read_file_vec};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;
use tar::Archive;

const ISO_BLOCK_SIZE: u64 = 2048;

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

        for line in reader.lines() {
            let line = line.map_err(|e| AppError::FileRead {
                path: path.clone(),
                source: e,
            })?;

            if let Some((key, value)) = parse_config_line(&line) {
                if key == "filename" {
                    filename = Some(value.to_string());
                }
            }
        }

        let filename = filename.ok_or_else(|| AppError::ConfigParse {
            path: path.clone(),
            message: "Missing required 'filename' field".to_string(),
        })?;

        Ok(IsoConfig { filename })
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

    /// Read a file from within an archive (ISO or tar.gz).
    ///
    /// Automatically detects the archive type based on filename extension.
    pub fn read_from_archive(&self, iso_name: &str, file_path: &str) -> AppResult<Vec<u8>> {
        let config = self.load_config(iso_name)?;

        if is_tarball(&config.filename) {
            self.read_from_tarball(iso_name, file_path)
        } else {
            self.read_from_iso(iso_name, file_path)
        }
    }

    /// Read a file from within an ISO.
    fn read_from_iso(&self, iso_name: &str, file_path: &str) -> AppResult<Vec<u8>> {
        let iso_path = self.iso_file_path(iso_name)?;

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
            tracing::debug!("File not found: {}", e);
            AppError::FileNotFoundInIso {
                iso: iso_name.to_string(),
                path: file_path.to_string(),
            }
        })?;

        read_file_vec(&mut block_io, &entry).map_err(|e| AppError::IsoRead {
            path: iso_path,
            message: format!("Failed to read file from ISO: {}", e),
        })
    }

    /// Read a file from within a tar.gz archive.
    fn read_from_tarball(&self, iso_name: &str, file_path: &str) -> AppResult<Vec<u8>> {
        let tarball_path = self.iso_file_path(iso_name)?;

        let file = File::open(&tarball_path).map_err(|e| AppError::FileRead {
            path: tarball_path.clone(),
            source: e,
        })?;

        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        // Normalize path - remove leading slash for tar matching
        let normalized_path = file_path.trim_start_matches('/');

        tracing::debug!("Looking for file in tarball: {}", normalized_path);

        let entries = archive.entries().map_err(|e| AppError::IsoRead {
            path: tarball_path.clone(),
            message: format!("Failed to read tarball entries: {}", e),
        })?;

        for entry in entries {
            let mut entry = entry.map_err(|e| AppError::IsoRead {
                path: tarball_path.clone(),
                message: format!("Failed to read tarball entry: {}", e),
            })?;

            let entry_path = entry.path().map_err(|e| AppError::IsoRead {
                path: tarball_path.clone(),
                message: format!("Failed to get entry path: {}", e),
            })?;

            let entry_path_str = entry_path.to_string_lossy();
            let entry_normalized = entry_path_str.trim_start_matches("./");

            if entry_normalized == normalized_path {
                let mut content = Vec::new();
                entry.read_to_end(&mut content).map_err(|e| AppError::IsoRead {
                    path: tarball_path.clone(),
                    message: format!("Failed to read file from tarball: {}", e),
                })?;
                return Ok(content);
            }
        }

        Err(AppError::FileNotFoundInIso {
            iso: iso_name.to_string(),
            path: file_path.to_string(),
        })
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

/// Check if a filename is a tarball (tar.gz or tgz).
fn is_tarball(filename: &str) -> bool {
    filename.ends_with(".tar.gz") || filename.ends_with(".tgz")
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
    fn test_is_tarball() {
        assert!(is_tarball("netboot.tar.gz"));
        assert!(is_tarball("ubuntu.tgz"));
        assert!(!is_tarball("ubuntu.iso"));
        assert!(!is_tarball("archive.tar"));
        assert!(!is_tarball("file.gz"));
    }

    #[test]
    fn test_read_from_tarball() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use tar::Builder;

        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("netboot");
        std::fs::create_dir_all(&iso_dir).unwrap();

        // Create iso.cfg pointing to tar.gz
        std::fs::write(iso_dir.join("iso.cfg"), "filename=netboot.tar.gz\n").unwrap();

        // Create a tar.gz file with test content
        let tarball_path = iso_dir.join("netboot.tar.gz");
        let file = File::create(&tarball_path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);

        // Add a file to the tarball
        let vmlinuz_content = b"vmlinuz binary content";
        let mut header = tar::Header::new_gnu();
        header.set_path("casper/vmlinuz").unwrap();
        header.set_size(vmlinuz_content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &vmlinuz_content[..]).unwrap();

        // Add another file
        let initrd_content = b"initrd binary content";
        let mut header = tar::Header::new_gnu();
        header.set_path("casper/initrd").unwrap();
        header.set_size(initrd_content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &initrd_content[..]).unwrap();

        builder.into_inner().unwrap().finish().unwrap();

        // Test reading from tarball
        let service = IsoService::new(dir.path().to_path_buf());

        let content = service.read_from_archive("netboot", "casper/vmlinuz").unwrap();
        assert_eq!(content, vmlinuz_content);

        let content = service.read_from_archive("netboot", "casper/initrd").unwrap();
        assert_eq!(content, initrd_content);
    }

    #[test]
    fn test_read_from_tarball_with_leading_slash() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use tar::Builder;

        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("netboot");
        std::fs::create_dir_all(&iso_dir).unwrap();

        std::fs::write(iso_dir.join("iso.cfg"), "filename=netboot.tar.gz\n").unwrap();

        let tarball_path = iso_dir.join("netboot.tar.gz");
        let file = File::create(&tarball_path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);

        let content = b"test content";
        let mut header = tar::Header::new_gnu();
        header.set_path("boot/vmlinuz").unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &content[..]).unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        let service = IsoService::new(dir.path().to_path_buf());

        // Should work with or without leading slash
        let result = service.read_from_archive("netboot", "/boot/vmlinuz").unwrap();
        assert_eq!(result, content);

        let result = service.read_from_archive("netboot", "boot/vmlinuz").unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn test_read_from_tarball_file_not_found() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use tar::Builder;

        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("netboot");
        std::fs::create_dir_all(&iso_dir).unwrap();

        std::fs::write(iso_dir.join("iso.cfg"), "filename=netboot.tar.gz\n").unwrap();

        // Create empty tarball
        let tarball_path = iso_dir.join("netboot.tar.gz");
        let file = File::create(&tarball_path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let builder = Builder::new(encoder);
        builder.into_inner().unwrap().finish().unwrap();

        let service = IsoService::new(dir.path().to_path_buf());

        let result = service.read_from_archive("netboot", "nonexistent");
        assert!(matches!(result, Err(AppError::FileNotFoundInIso { .. })));
    }
}
