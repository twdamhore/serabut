use std::collections::HashMap;
use std::path::Path;

use crate::error::AppError;

#[derive(Debug)]
pub struct ActionEntry {
    pub release: String,
    pub automation: String,
}

#[derive(Debug)]
pub struct ActionConfig {
    entries: HashMap<String, ActionEntry>,
    path: std::path::PathBuf,
}

impl ActionConfig {
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let mut entries = HashMap::new();

        if !path.exists() {
            return Ok(ActionConfig {
                entries,
                path: path.to_path_buf(),
            });
        }

        let content = std::fs::read_to_string(path)?;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((hostname, rest)) = line.split_once('=') {
                let hostname = hostname.trim().to_string();
                let parts: Vec<&str> = rest.split(',').collect();
                let release = parts[0].trim().to_string();
                let automation = parts.get(1).map(|s| s.trim()).unwrap_or("default").to_string();

                entries.insert(
                    hostname,
                    ActionEntry {
                        release,
                        automation,
                    },
                );
            }
        }

        Ok(ActionConfig {
            entries,
            path: path.to_path_buf(),
        })
    }

    pub fn get(&self, hostname: &str) -> Option<(String, String)> {
        self.entries
            .get(hostname)
            .map(|e| (e.release.clone(), e.automation.clone()))
    }

    pub fn has_entry(&self, hostname: &str) -> bool {
        self.entries.contains_key(hostname)
    }

    /// Mark installation complete by commenting out the hostname entry
    pub fn mark_done(&mut self, hostname: &str) -> Result<(), AppError> {
        if !self.path.exists() {
            return Err(AppError::NotFound("action.cfg not found".to_string()));
        }

        let content = std::fs::read_to_string(&self.path)?;
        let mut new_lines = Vec::new();
        let mut found = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                if let Some((h, _)) = trimmed.split_once('=') {
                    if h.trim() == hostname {
                        new_lines.push(format!("# {}", line));
                        found = true;
                        continue;
                    }
                }
            }
            new_lines.push(line.to_string());
        }

        if !found {
            return Err(AppError::NotFound(format!(
                "Hostname '{}' not found in action.cfg",
                hostname
            )));
        }

        std::fs::write(&self.path, new_lines.join("\n") + "\n")?;

        // Remove from in-memory entries
        self.entries.remove(hostname);

        Ok(())
    }
}
