use std::collections::HashMap;
use std::path::Path;

use axum::body::Body;
use futures::stream::{self, StreamExt};
use tokio::task;

use crate::error::AppError;
use crate::services::iso;

#[derive(Debug, Clone)]
pub enum CombineSource {
    /// Read from inside ISO: content:{release}/{path}
    Content { release: String, path: String },
    /// Read from filesystem: file:{relative_path}
    File { path: String },
}

#[derive(Debug, Clone)]
pub struct CombineEntry {
    pub sources: Vec<CombineSource>,
}

#[derive(Debug)]
pub struct CombineConfig {
    entries: HashMap<String, CombineEntry>,
}

impl CombineConfig {
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let mut entries = HashMap::new();

        if !path.exists() {
            return Ok(CombineConfig { entries });
        }

        let content = std::fs::read_to_string(path)?;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((name, sources_str)) = line.split_once('=') {
                let name = name.trim().to_string();
                let mut sources = Vec::new();

                for source in sources_str.split(',') {
                    let source = source.trim();
                    if let Some(content_path) = source.strip_prefix("content:") {
                        // Parse release/path
                        if let Some((release, path)) = content_path.split_once('/') {
                            sources.push(CombineSource::Content {
                                release: release.to_string(),
                                path: path.to_string(),
                            });
                        }
                    } else if let Some(file_path) = source.strip_prefix("file:") {
                        sources.push(CombineSource::File {
                            path: file_path.to_string(),
                        });
                    }
                }

                if !sources.is_empty() {
                    entries.insert(name, CombineEntry { sources });
                }
            }
        }

        Ok(CombineConfig { entries })
    }

    pub fn get(&self, name: &str) -> Option<&CombineEntry> {
        self.entries.get(name)
    }
}

/// Calculate total size of combined sources (sync version for use in spawn_blocking)
fn calculate_combined_size_sync(
    sources: &[(CombineSource, std::path::PathBuf)],
) -> Result<u64, AppError> {
    let mut total = 0u64;

    for (source, resolved_path) in sources {
        match source {
            CombineSource::Content { path, .. } => {
                total += iso::get_file_size(resolved_path, path)?;
            }
            CombineSource::File { .. } => {
                let metadata = std::fs::metadata(resolved_path)?;
                total += metadata.len();
            }
        }
    }

    Ok(total)
}

/// Stream combined sources sequentially (async version)
pub async fn stream_combined(
    entry: &CombineEntry,
    iso_dir: &Path,
    aliases: &crate::services::aliases::AliasesConfig,
) -> Result<(u64, Body), AppError> {
    // Pre-resolve all paths to owned data
    let resolved_sources: Vec<(CombineSource, std::path::PathBuf)> = entry
        .sources
        .iter()
        .map(|source| {
            match source {
                CombineSource::Content { release, path: _ } => {
                    let filename = aliases
                        .get_filename(release)
                        .ok_or_else(|| AppError::NotFound(format!("Unknown release: {}", release)))?;
                    Ok((source.clone(), iso_dir.join(filename)))
                }
                CombineSource::File { path } => {
                    Ok((source.clone(), iso_dir.join(path)))
                }
            }
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    // Calculate size using spawn_blocking for sync I/O
    let sources_for_size = resolved_sources.clone();
    let size = task::spawn_blocking(move || {
        calculate_combined_size_sync(&sources_for_size)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let stream = stream::iter(resolved_sources)
        .then(move |(source, resolved_path)| async move {
            match source {
                CombineSource::Content { path, .. } => {
                    // Read ISO content using spawn_blocking for sync I/O
                    let result = task::spawn_blocking(move || {
                        iso::read_file(&resolved_path, &path)
                    })
                    .await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

                    result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                }
                CombineSource::File { .. } => {
                    // Read filesystem file
                    tokio::fs::read(&resolved_path).await
                }
            }
        })
        .map(|result| result.map(bytes::Bytes::from));

    Ok((size, Body::from_stream(stream)))
}
