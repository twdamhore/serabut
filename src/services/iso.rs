//! ISO service for managing ISO files and reading their contents.
//!
//! Handles iso.cfg parsing, ISO9660 reading, and template detection.

use crate::error::{AppError, AppResult};
use iso9660_simple::{ISODirectoryEntry, Read as IsoRead, ISO9660};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Wrapper to implement iso9660_simple::Read for std::fs::File.
struct FileDevice(File);

impl IsoRead for FileDevice {
    fn read(&mut self, position: usize, buffer: &mut [u8]) -> Option<()> {
        if self.0.seek(SeekFrom::Start(position as u64)).is_err() {
            return None;
        }
        if self.0.read_exact(buffer).is_ok() {
            Some(())
        } else {
            None
        }
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
    ///
    /// Checks:
    /// - iso/ directory exists
    /// - iso/ has at least one subdirectory
    /// - Each subdirectory has iso.cfg
    /// - iso.cfg contains filename= reference
    /// - Referenced ISO file exists and is readable
    pub fn validate_startup(&self) {
        let iso_dir = self.config_path.join("iso");

        // Check if iso directory exists
        if !iso_dir.exists() {
            tracing::warn!(
                "ISO directory does not exist: {:?}. \
                Create this directory and add ISO subdirectories (e.g., ubuntu-24.04.3/) \
                to enable PXE boot functionality.",
                iso_dir
            );
            return;
        }

        // Check if iso directory has any subdirectories
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

        // Validate each ISO subdirectory
        for entry in subdirs {
            let iso_name = entry.file_name();
            let iso_name_str = iso_name.to_string_lossy();
            self.validate_iso_subdir(&iso_name_str, &entry.path());
        }
    }

    /// Validate a single ISO subdirectory.
    fn validate_iso_subdir(&self, iso_name: &str, iso_path: &std::path::Path) {
        let iso_cfg_path = iso_path.join("iso.cfg");

        // Check if iso.cfg exists
        if !iso_cfg_path.exists() {
            tracing::warn!(
                "ISO '{}': missing iso.cfg at {:?}. \
                Create this file with 'filename=<iso-file-name>' to specify the ISO file.",
                iso_name,
                iso_cfg_path
            );
            return;
        }

        // Try to parse iso.cfg and check for filename
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

        // Look for filename= line
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

        // Check if the referenced ISO file exists
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

        // Check if the ISO file is readable
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

        // Check for boot.ipxe.j2 template
        let boot_template = iso_path.join("boot.ipxe.j2");
        if !boot_template.exists() {
            tracing::warn!(
                "ISO '{}': missing boot.ipxe.j2 at {:?}. \
                See https://github.com/twdamhore/serabut#directory-structure for template examples.",
                iso_name,
                boot_template
            );
        }

        // Check for automation directory
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
            // Check if automation directory has any profiles
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

    /// Get the path to an ISO directory.
    fn iso_dir(&self, iso_name: &str) -> PathBuf {
        self.config_path.join("iso").join(iso_name)
    }

    /// Get the path to iso.cfg for an ISO.
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
    /// Templates have .j2 extension added to the path.
    pub fn template_path(&self, iso_name: &str, path: &str) -> Option<PathBuf> {
        let template_path = self.iso_dir(iso_name).join(format!("{}.j2", path));
        if template_path.exists() {
            Some(template_path)
        } else {
            None
        }
    }

    /// Read a file from within an ISO using iso9660_simple.
    pub fn read_from_iso(&self, iso_name: &str, file_path: &str) -> AppResult<Vec<u8>> {
        let iso_path = self.iso_file_path(iso_name)?;

        let file = File::open(&iso_path).map_err(|e| AppError::FileRead {
            path: iso_path.clone(),
            source: e,
        })?;

        let mut iso = ISO9660::from_device(FileDevice(file)).ok_or_else(|| AppError::IsoRead {
            path: iso_path.clone(),
            message: "Failed to parse ISO9660 filesystem".to_string(),
        })?;

        // Normalize path - remove leading slash if present
        let normalized_path = file_path.trim_start_matches('/');

        let entry = find_file_in_iso(&mut iso, normalized_path).ok_or_else(|| {
            AppError::FileNotFoundInIso {
                iso: iso_name.to_string(),
                path: file_path.to_string(),
            }
        })?;

        read_iso_file(&mut iso, &entry).map_err(|e| AppError::IsoRead {
            path: iso_path,
            message: e,
        })
    }

    /// Get the boot template path for an ISO.
    pub fn boot_template_path(&self, iso_name: &str) -> AppResult<PathBuf> {
        let path = self.iso_dir(iso_name).join("boot.ipxe.j2");
        if !path.exists() {
            return Err(AppError::TemplateNotFound { path });
        }
        Ok(path)
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

/// Find a file in an ISO by path.
fn find_file_in_iso(iso: &mut ISO9660, path: &str) -> Option<ISODirectoryEntry> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if parts.is_empty() {
        return None;
    }

    find_entry_recursive(iso, iso.root().lba.lsb, &parts)
}

/// Recursively find an entry in the ISO directory structure.
fn find_entry_recursive(
    iso: &mut ISO9660,
    dir_lba: u32,
    remaining_parts: &[&str],
) -> Option<ISODirectoryEntry> {
    if remaining_parts.is_empty() {
        return None;
    }

    let target = remaining_parts[0];
    let rest = &remaining_parts[1..];

    // Collect directory entries first to avoid borrow issues with recursion
    let entries: Vec<ISODirectoryEntry> = {
        let dir_iter = iso.read_directory(dir_lba as usize);
        (&dir_iter).collect()
    };

    for entry in entries {
        // ISO9660 names might have version suffix (;1) - strip it
        let clean_name = entry.name.split(';').next().unwrap_or(&entry.name);

        if clean_name.eq_ignore_ascii_case(target) {
            if rest.is_empty() {
                // Found the target
                return Some(entry);
            } else if entry.is_folder() {
                // Continue searching in subdirectory
                return find_entry_recursive(iso, entry.lsb_position(), rest);
            }
        }
    }

    None
}

/// Read the contents of an ISO file entry.
fn read_iso_file(iso: &mut ISO9660, entry: &ISODirectoryEntry) -> Result<Vec<u8>, String> {
    if entry.is_folder() {
        return Err("Path is a directory, not a file".to_string());
    }

    let size = entry.file_size() as usize;
    let mut contents = vec![0u8; size];

    iso.read_file(entry, 0, &mut contents)
        .ok_or_else(|| "Failed to read file from ISO".to_string())?;

    Ok(contents)
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
    fn test_boot_template_path() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        std::fs::create_dir_all(&iso_dir).unwrap();
        std::fs::write(iso_dir.join("boot.ipxe.j2"), "boot template").unwrap();

        let service = IsoService::new(dir.path().to_path_buf());
        let path = service.boot_template_path("ubuntu-24.04").unwrap();

        assert!(path.exists());
    }

    #[test]
    fn test_boot_template_path_not_found() {
        let dir = setup_test_dir();
        let iso_dir = dir.path().join("iso").join("ubuntu-24.04");
        std::fs::create_dir_all(&iso_dir).unwrap();

        let service = IsoService::new(dir.path().to_path_buf());
        let result = service.boot_template_path("ubuntu-24.04");

        assert!(matches!(result, Err(AppError::TemplateNotFound { .. })));
    }
}
