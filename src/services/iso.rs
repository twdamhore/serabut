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
