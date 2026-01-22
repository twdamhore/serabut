use std::collections::HashMap;
use std::path::Path;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct AliasEntry {
    pub filename: String,
    pub downloadable: bool,
}

#[derive(Debug)]
pub struct AliasesConfig {
    entries: HashMap<String, AliasEntry>,
}

impl AliasesConfig {
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let mut entries = HashMap::new();

        if !path.exists() {
            return Ok(AliasesConfig { entries });
        }

        let content = std::fs::read_to_string(path)?;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((release, rest)) = line.split_once('=') {
                let release = release.trim().to_string();
                let parts: Vec<&str> = rest.split(',').collect();
                let filename = parts[0].trim().to_string();
                let downloadable = parts.get(1).map(|s| s.trim() == "downloadable").unwrap_or(false);

                entries.insert(
                    release,
                    AliasEntry {
                        filename,
                        downloadable,
                    },
                );
            }
        }

        Ok(AliasesConfig { entries })
    }

    pub fn get_filename(&self, release: &str) -> Option<&str> {
        self.entries.get(release).map(|e| e.filename.as_str())
    }

    pub fn is_downloadable(&self, release: &str) -> bool {
        self.entries.get(release).map(|e| e.downloadable).unwrap_or(false)
    }
}
