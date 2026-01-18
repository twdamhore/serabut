//! Action service for managing action.cfg.
//!
//! Handles reading MAC entries and marking them as completed with file locking.

use crate::error::{AppError, AppResult};
use chrono::Utc;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Represents a pending action for a MAC address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub mac: String,
    pub iso: String,
    pub automation: String,
}

/// Service for managing action.cfg file.
pub struct ActionService {
    config_path: PathBuf,
}

impl ActionService {
    /// Create a new action service.
    pub fn new(config_path: PathBuf) -> Self {
        Self { config_path }
    }

    /// Get the path to action.cfg.
    fn action_cfg_path(&self) -> PathBuf {
        self.config_path.join("action.cfg")
    }

    /// Look up a MAC address in action.cfg.
    ///
    /// Returns None if MAC is not found or is commented out.
    pub fn lookup(&self, mac: &str) -> AppResult<Option<Action>> {
        let path = self.action_cfg_path();

        if !path.exists() {
            return Ok(None);
        }

        let file = File::open(&path).map_err(|e| AppError::FileRead {
            path: path.clone(),
            source: e,
        })?;

        // Shared lock for reading
        FileExt::lock_shared(&file).map_err(|e| AppError::FileRead {
            path: path.clone(),
            source: e,
        })?;

        let reader = BufReader::new(&file);
        for line in reader.lines() {
            let line = line.map_err(|e| AppError::FileRead {
                path: path.clone(),
                source: e,
            })?;

            if let Some(action) = parse_action_line(&line, mac) {
                return Ok(Some(action));
            }
        }

        Ok(None)
    }

    /// Mark a MAC address as completed in action.cfg.
    ///
    /// Adds a completion timestamp comment and comments out the original line.
    pub fn mark_completed(&self, mac: &str) -> AppResult<bool> {
        let path = self.action_cfg_path();

        if !path.exists() {
            return Ok(false);
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| AppError::FileRead {
                path: path.clone(),
                source: e,
            })?;

        // Exclusive lock for writing
        file.lock_exclusive().map_err(|e| AppError::FileWrite {
            path: path.clone(),
            source: e,
        })?;

        let reader = BufReader::new(&file);
        let lines: Vec<String> = reader
            .lines()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::FileRead {
                path: path.clone(),
                source: e,
            })?;

        let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%S-UTC");
        let mut modified = false;
        let mut new_lines = Vec::with_capacity(lines.len() + 1);

        for line in lines {
            if !modified && is_mac_line(&line, mac) {
                // Add completion comment and commented-out original line
                new_lines.push(format!("# completed {} on {}", mac, timestamp));
                new_lines.push(format!("# {}", line));
                modified = true;
            } else {
                new_lines.push(line);
            }
        }

        if modified {
            write_lines_to_file(&path, &new_lines)?;
            tracing::info!("Marked MAC {} as completed", mac);
        }

        Ok(modified)
    }
}

/// Parse a line from action.cfg looking for a specific MAC.
fn parse_action_line(line: &str, target_mac: &str) -> Option<Action> {
    let line = line.trim();

    // Skip comments and empty lines
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    // Format: mac=iso,automation
    let (mac, rest) = line.split_once('=')?;
    let mac = mac.trim();

    if !mac.eq_ignore_ascii_case(target_mac) {
        return None;
    }

    let (iso, automation) = rest.split_once(',')?;

    Some(Action {
        mac: mac.to_string(),
        iso: iso.trim().to_string(),
        automation: automation.trim().to_string(),
    })
}

/// Check if a line is an active (non-commented) entry for the given MAC.
fn is_mac_line(line: &str, target_mac: &str) -> bool {
    let line = line.trim();

    if line.is_empty() || line.starts_with('#') {
        return false;
    }

    if let Some((mac, _)) = line.split_once('=') {
        return mac.trim().eq_ignore_ascii_case(target_mac);
    }

    false
}

/// Write lines to a file, truncating it first.
fn write_lines_to_file(path: &Path, lines: &[String]) -> AppResult<()> {
    let content = lines.join("\n") + "\n";

    std::fs::write(path, content).map_err(|e| AppError::FileWrite {
        path: path.to_path_buf(),
        source: e,
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_parse_action_line() {
        let action = parse_action_line("aa-bb-cc-dd-ee-ff=ubuntu-24.04,docker", "aa-bb-cc-dd-ee-ff");
        assert_eq!(
            action,
            Some(Action {
                mac: "aa-bb-cc-dd-ee-ff".to_string(),
                iso: "ubuntu-24.04".to_string(),
                automation: "docker".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_action_line_wrong_mac() {
        let action = parse_action_line("aa-bb-cc-dd-ee-ff=ubuntu-24.04,docker", "11-22-33-44-55-66");
        assert_eq!(action, None);
    }

    #[test]
    fn test_parse_action_line_comment() {
        let action =
            parse_action_line("# aa-bb-cc-dd-ee-ff=ubuntu-24.04,docker", "aa-bb-cc-dd-ee-ff");
        assert_eq!(action, None);
    }

    #[test]
    fn test_is_mac_line() {
        assert!(is_mac_line("aa-bb-cc-dd-ee-ff=ubuntu,docker", "aa-bb-cc-dd-ee-ff"));
        assert!(!is_mac_line("# aa-bb-cc-dd-ee-ff=ubuntu,docker", "aa-bb-cc-dd-ee-ff"));
        assert!(!is_mac_line("11-22-33-44-55-66=ubuntu,docker", "aa-bb-cc-dd-ee-ff"));
    }

    #[test]
    fn test_lookup_mac_not_found() {
        let dir = setup_test_dir();
        std::fs::write(
            dir.path().join("action.cfg"),
            "11-22-33-44-55-66=ubuntu,minimal\n",
        )
        .unwrap();

        let service = ActionService::new(dir.path().to_path_buf());
        let result = service.lookup("aa-bb-cc-dd-ee-ff").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_lookup_mac_found() {
        let dir = setup_test_dir();
        std::fs::write(
            dir.path().join("action.cfg"),
            "aa-bb-cc-dd-ee-ff=ubuntu-24.04,docker\n",
        )
        .unwrap();

        let service = ActionService::new(dir.path().to_path_buf());
        let result = service.lookup("aa-bb-cc-dd-ee-ff").unwrap();
        assert_eq!(
            result,
            Some(Action {
                mac: "aa-bb-cc-dd-ee-ff".to_string(),
                iso: "ubuntu-24.04".to_string(),
                automation: "docker".to_string(),
            })
        );
    }

    #[test]
    fn test_mark_completed() {
        let dir = setup_test_dir();
        std::fs::write(
            dir.path().join("action.cfg"),
            "aa-bb-cc-dd-ee-ff=ubuntu-24.04,docker\n11-22-33-44-55-66=alma,minimal\n",
        )
        .unwrap();

        let service = ActionService::new(dir.path().to_path_buf());
        let result = service.mark_completed("aa-bb-cc-dd-ee-ff").unwrap();
        assert!(result);

        let content = std::fs::read_to_string(dir.path().join("action.cfg")).unwrap();
        assert!(content.contains("# completed aa-bb-cc-dd-ee-ff on"));
        assert!(content.contains("# aa-bb-cc-dd-ee-ff=ubuntu-24.04,docker"));
        assert!(content.contains("11-22-33-44-55-66=alma,minimal"));

        // Should not find it anymore
        let lookup = service.lookup("aa-bb-cc-dd-ee-ff").unwrap();
        assert!(lookup.is_none());
    }

    #[test]
    fn test_mark_completed_not_found() {
        let dir = setup_test_dir();
        std::fs::write(
            dir.path().join("action.cfg"),
            "11-22-33-44-55-66=alma,minimal\n",
        )
        .unwrap();

        let service = ActionService::new(dir.path().to_path_buf());
        let result = service.mark_completed("aa-bb-cc-dd-ee-ff").unwrap();
        assert!(!result);
    }
}
