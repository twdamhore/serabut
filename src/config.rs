//! Configuration management for serabutd.
//!
//! Handles parsing of /etc/serabutd.conf and runtime configuration.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Interface to bind to (default: 0.0.0.0)
    pub interface: IpAddr,
    /// Port to listen on (default: 4123)
    pub port: u16,
    /// Log level (default: info)
    pub log_level: LogLevel,
    /// Base path for config files (default: /var/lib/serabutd/config)
    pub config_path: PathBuf,
}

/// Log level configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interface: IpAddr::from([0, 0, 0, 0]),
            port: 4123,
            log_level: LogLevel::Info,
            config_path: PathBuf::from("/var/lib/serabutd/config"),
        }
    }
}

impl Config {
    /// Load configuration from file.
    ///
    /// If the file doesn't exist, returns default configuration.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            tracing::info!("Config file not found at {:?}, using defaults", path);
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::ReadError {
            path: path.to_path_buf(),
            source: e,
        })?;

        Self::parse(&content, path)
    }

    /// Parse configuration from string content.
    fn parse(content: &str, path: &Path) -> Result<Self, ConfigError> {
        let mut config = Self::default();

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let (key, value) = parse_key_value(line).ok_or_else(|| ConfigError::ParseError {
                path: path.to_path_buf(),
                line: line_num + 1,
                message: format!("Invalid line format: {}", line),
            })?;

            match key {
                "interface" => {
                    config.interface =
                        IpAddr::from_str(value).map_err(|_| ConfigError::ParseError {
                            path: path.to_path_buf(),
                            line: line_num + 1,
                            message: format!("Invalid interface address: {}", value),
                        })?;
                }
                "port" => {
                    config.port = value.parse().map_err(|_| ConfigError::ParseError {
                        path: path.to_path_buf(),
                        line: line_num + 1,
                        message: format!("Invalid port number: {}", value),
                    })?;
                }
                "log_level" => {
                    config.log_level =
                        LogLevel::from_str(value).map_err(|_| ConfigError::ParseError {
                            path: path.to_path_buf(),
                            line: line_num + 1,
                            message: format!("Invalid log level: {}", value),
                        })?;
                }
                "config_path" => {
                    config.config_path = PathBuf::from(value);
                }
                _ => {
                    tracing::warn!("Unknown config key '{}' at line {}", key, line_num + 1);
                }
            }
        }

        Ok(config)
    }

    /// Get the tracing filter string for this log level.
    pub fn tracing_filter(&self) -> &'static str {
        match self.log_level {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
        }
    }
}

impl FromStr for LogLevel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "error" => Ok(LogLevel::Error),
            "warn" | "warning" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            _ => Err(()),
        }
    }
}

/// Parse a key=value line.
fn parse_key_value(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.splitn(2, '=');
    let key = parts.next()?.trim();
    let value = parts.next()?.trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}

/// Configuration error types.
#[derive(Debug)]
pub enum ConfigError {
    ReadError {
        path: PathBuf,
        source: std::io::Error,
    },
    ParseError {
        path: PathBuf,
        line: usize,
        message: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::ReadError { path, source } => {
                write!(f, "Failed to read config file {:?}: {}", path, source)
            }
            ConfigError::ParseError {
                path,
                line,
                message,
            } => {
                write!(f, "Config parse error in {:?} at line {}: {}", path, line, message)
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Shared application state that can be reloaded.
#[derive(Clone)]
pub struct AppState {
    config: Arc<RwLock<Config>>,
    config_path: PathBuf,
}

impl AppState {
    /// Create new application state from config file path.
    pub fn new(config_path: PathBuf) -> Result<Self, ConfigError> {
        let config = Config::load(&config_path)?;
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
        })
    }

    /// Get current configuration.
    pub async fn config(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Reload configuration from disk.
    pub async fn reload(&self) -> Result<(), ConfigError> {
        let new_config = Config::load(&self.config_path)?;
        let mut config = self.config.write().await;
        *config = new_config;
        tracing::info!("Configuration reloaded");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.interface, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(config.port, 4123);
        assert_eq!(config.log_level, LogLevel::Info);
    }

    #[test]
    fn test_parse_config() {
        let content = r#"
            interface=192.168.1.1
            port=8080
            log_level=debug
        "#;
        let config = Config::parse(content, Path::new("test.conf")).unwrap();
        assert_eq!(config.interface, IpAddr::from([192, 168, 1, 1]));
        assert_eq!(config.port, 8080);
        assert_eq!(config.log_level, LogLevel::Debug);
    }

    #[test]
    fn test_parse_config_with_comments() {
        let content = r#"
            # This is a comment
            interface=10.0.0.1
            # Another comment
            port=9000
        "#;
        let config = Config::parse(content, Path::new("test.conf")).unwrap();
        assert_eq!(config.interface, IpAddr::from([10, 0, 0, 1]));
        assert_eq!(config.port, 9000);
    }

    #[test]
    fn test_parse_key_value() {
        assert_eq!(parse_key_value("key=value"), Some(("key", "value")));
        assert_eq!(parse_key_value("key = value"), Some(("key", "value")));
        assert_eq!(parse_key_value("key="), Some(("key", "")));
        assert_eq!(parse_key_value("=value"), None);
        assert_eq!(parse_key_value("no equals"), None);
    }

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(LogLevel::from_str("error"), Ok(LogLevel::Error));
        assert_eq!(LogLevel::from_str("WARN"), Ok(LogLevel::Warn));
        assert_eq!(LogLevel::from_str("Info"), Ok(LogLevel::Info));
        assert_eq!(LogLevel::from_str("debug"), Ok(LogLevel::Debug));
        assert!(LogLevel::from_str("invalid").is_err());
    }
}
