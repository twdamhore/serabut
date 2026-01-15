use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use thiserror::Error;

pub const MAC_FILE: &str = "/var/lib/serabut/mac.txt";
pub const BOOT_FILE: &str = "/var/lib/serabut/boot.txt";
pub const PROFILES_DIR: &str = "/etc/serabut/profiles";
pub const DATA_DIR: &str = "/var/lib/serabut";

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
            mac,
            last_seen: Utc::now(),
        }
    }

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

pub fn normalize_mac(mac: &str) -> String {
    mac.to_lowercase()
}

pub fn ensure_data_dir() -> Result<()> {
    fs::create_dir_all(DATA_DIR).context("Failed to create data directory")?;
    Ok(())
}

pub fn read_mac_entries() -> Result<Vec<MacEntry>> {
    let path = Path::new(MAC_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path).context("Failed to open mac.txt")?;
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

pub fn write_mac_entries(entries: &[MacEntry]) -> Result<()> {
    ensure_data_dir()?;

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(MAC_FILE)
        .context("Failed to open mac.txt for writing")?;

    for entry in entries {
        writeln!(file, "{}", entry.to_csv_line())?;
    }

    Ok(())
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
    let path = Path::new(PROFILES_DIR);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut profiles = Vec::new();
    for entry in fs::read_dir(path).context("Failed to read profiles directory")? {
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
    let path = Path::new(PROFILES_DIR).join(format!("{}.ipxe", name));
    path.exists()
}
