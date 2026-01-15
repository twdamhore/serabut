use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use thiserror::Error;

// File locking
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

/// Get the data directory, configurable via SERABUT_DATA_DIR env var
pub fn data_dir() -> PathBuf {
    env::var("SERABUT_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/serabut"))
}

/// Get the config directory, configurable via SERABUT_CONFIG_DIR env var
pub fn config_dir() -> PathBuf {
    env::var("SERABUT_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/serabut"))
}

/// Get the MAC file path
pub fn mac_file_path() -> PathBuf {
    data_dir().join("mac.txt")
}

/// Get the boot file path
pub fn boot_file_path() -> PathBuf {
    data_dir().join("boot.txt")
}

/// Get the profiles directory path
pub fn profiles_dir() -> PathBuf {
    config_dir().join("profiles")
}

#[derive(Error, Debug)]
pub enum SerabutError {
    #[error("MAC address '{0}' not found")]
    MacNotFound(String),

    #[error("Label '{label}' already taken by {mac}")]
    LabelTaken { label: String, mac: String },

    #[error("Invalid label '{0}': must be a-z only, max 8 characters")]
    InvalidLabel(String),

    #[error("Invalid MAC address format: {0}")]
    InvalidMac(String),

    #[error("Profile '{0}' not found")]
    ProfileNotFound(String),
}

#[derive(Debug, Clone)]
pub struct MacEntry {
    pub label: String,
    pub mac: String,
    pub last_seen: DateTime<Utc>,
}

impl MacEntry {
    pub fn new(mac: String) -> Self {
        Self {
            label: String::new(),
            mac: normalize_mac(&mac),
            last_seen: Utc::now(),
        }
    }

    /// Parse a MacEntry from a CSV line.
    /// Format: label,mac,timestamp
    /// Note: Labels are validated to be a-z only, so commas in labels are not possible.
    pub fn from_csv_line(line: &str) -> Result<Self> {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != 3 {
            return Err(anyhow!("Invalid CSV line: {}", line));
        }

        let last_seen = DateTime::parse_from_rfc3339(parts[2])
            .context("Invalid timestamp")?
            .with_timezone(&Utc);

        Ok(Self {
            label: parts[0].to_string(),
            mac: parts[1].to_string(),
            last_seen,
        })
    }

    pub fn to_csv_line(&self) -> String {
        format!(
            "{},{},{}",
            self.label,
            self.mac,
            self.last_seen.to_rfc3339()
        )
    }
}

/// Validate a label: must be empty or a-z only, max 8 characters
#[must_use = "validation result must be checked"]
pub fn validate_label(label: &str) -> Result<(), SerabutError> {
    if label.is_empty() {
        return Ok(());
    }
    if label.len() > 8 {
        return Err(SerabutError::InvalidLabel(label.to_string()));
    }
    if !label.chars().all(|c| c.is_ascii_lowercase()) {
        return Err(SerabutError::InvalidLabel(label.to_string()));
    }
    Ok(())
}

/// Validate a MAC address: must be in format aa:bb:cc:dd:ee:ff
#[must_use = "validation result must be checked"]
pub fn validate_mac(mac: &str) -> Result<(), SerabutError> {
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() != 6 {
        return Err(SerabutError::InvalidMac(mac.to_string()));
    }
    for part in parts {
        if part.len() != 2 {
            return Err(SerabutError::InvalidMac(mac.to_string()));
        }
        if !part.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(SerabutError::InvalidMac(mac.to_string()));
        }
    }
    Ok(())
}

/// Normalize a MAC address to lowercase
pub fn normalize_mac(mac: &str) -> String {
    mac.to_lowercase()
}

/// Ensure the data directory exists
pub fn ensure_data_dir() -> Result<()> {
    fs::create_dir_all(data_dir()).context("Failed to create data directory")?;
    Ok(())
}

/// Acquire an exclusive lock on a file (Unix only)
#[cfg(unix)]
fn lock_file_exclusive(file: &File) -> Result<()> {
    use libc::{flock, LOCK_EX};
    let fd = file.as_raw_fd();
    let result = unsafe { flock(fd, LOCK_EX) };
    if result != 0 {
        return Err(anyhow!("Failed to acquire file lock"));
    }
    Ok(())
}

/// Release a file lock (Unix only)
#[cfg(unix)]
fn unlock_file(file: &File) -> Result<()> {
    use libc::{flock, LOCK_UN};
    let fd = file.as_raw_fd();
    let result = unsafe { flock(fd, LOCK_UN) };
    if result != 0 {
        return Err(anyhow!("Failed to release file lock"));
    }
    Ok(())
}

/// No-op lock for non-Unix platforms
#[cfg(not(unix))]
fn lock_file_exclusive(_file: &File) -> Result<()> {
    Ok(())
}

/// No-op unlock for non-Unix platforms
#[cfg(not(unix))]
fn unlock_file(_file: &File) -> Result<()> {
    Ok(())
}

/// Read MAC entries from the mac.txt file
pub fn read_mac_entries() -> Result<Vec<MacEntry>> {
    let path = mac_file_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(&path).context("Failed to open mac.txt")?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = line.context("Failed to read line")?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        entries.push(MacEntry::from_csv_line(line)?);
    }

    Ok(entries)
}

/// Write MAC entries to the mac.txt file with file locking
pub fn write_mac_entries(entries: &[MacEntry]) -> Result<()> {
    ensure_data_dir()?;

    let path = mac_file_path();
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .context("Failed to open mac.txt for writing")?;

    lock_file_exclusive(&file)?;

    let mut writer = std::io::BufWriter::new(&file);
    for entry in entries {
        writeln!(writer, "{}", entry.to_csv_line())?;
    }
    writer.flush()?;

    unlock_file(&file)?;

    Ok(())
}

/// Read and write MAC entries atomically with file locking.
/// This prevents race conditions between concurrent readers/writers.
pub fn with_mac_entries<F, T>(f: F) -> Result<T>
where
    F: FnOnce(&mut Vec<MacEntry>) -> Result<T>,
{
    ensure_data_dir()?;

    let path = mac_file_path();

    // Open or create the file for read+write
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&path)
        .context("Failed to open mac.txt")?;

    lock_file_exclusive(&file)?;

    // Read existing entries
    let reader = BufReader::new(&file);
    let mut entries = Vec::new();
    for line in reader.lines() {
        let line = line.context("Failed to read line")?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        entries.push(MacEntry::from_csv_line(line)?);
    }

    // Apply the modification
    let result = f(&mut entries)?;

    // Truncate and rewrite
    let file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&path)
        .context("Failed to open mac.txt for writing")?;

    let mut writer = std::io::BufWriter::new(&file);
    for entry in &entries {
        writeln!(writer, "{}", entry.to_csv_line())?;
    }
    writer.flush()?;

    unlock_file(&file)?;

    Ok(result)
}

pub fn find_entry_by_mac(entries: &[MacEntry], mac: &str) -> Option<usize> {
    let mac = normalize_mac(mac);
    entries.iter().position(|e| e.mac == mac)
}

pub fn find_entry_by_label(entries: &[MacEntry], label: &str) -> Option<usize> {
    if label.is_empty() {
        return None;
    }
    entries.iter().position(|e| e.label == label)
}

pub fn update_or_insert_mac(entries: &mut Vec<MacEntry>, mac: &str) {
    let mac = normalize_mac(mac);
    if let Some(idx) = find_entry_by_mac(entries, &mac) {
        entries[idx].last_seen = Utc::now();
    } else {
        entries.push(MacEntry::new(mac));
    }
}

pub fn list_profiles() -> Result<Vec<String>> {
    let path = profiles_dir();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut profiles = Vec::new();
    for entry in fs::read_dir(&path).context("Failed to read profiles directory")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "ipxe") {
            if let Some(stem) = path.file_stem() {
                profiles.push(stem.to_string_lossy().to_string());
            }
        }
    }
    profiles.sort();
    Ok(profiles)
}

pub fn profile_exists(name: &str) -> bool {
    let path = profiles_dir().join(format!("{}.ipxe", name));
    path.exists()
}
