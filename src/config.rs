use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::error::AppError;
use crate::services::action::ActionConfig;
use crate::services::aliases::AliasesConfig;
use crate::services::combine::CombineConfig;
use crate::services::hardware::HardwareConfig;

const DEFAULT_CONFIG_PATH: &str = "/etc/serabutd.conf";
const DEFAULT_DATA_DIR: &str = "/var/lib/serabutd";
const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0";
const DEFAULT_PORT: u16 = 8080;

#[derive(Debug)]
pub struct Config {
    pub data_dir: PathBuf,
    pub bind_address: String,
    pub port: u16,
}

impl Config {
    pub fn load() -> Result<Self, AppError> {
        let config_path = std::env::var("SERABUTD_CONFIG")
            .unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());

        let mut data_dir = PathBuf::from(DEFAULT_DATA_DIR);
        let mut bind_address = DEFAULT_BIND_ADDRESS.to_string();
        let mut port = DEFAULT_PORT;

        if Path::new(&config_path).exists() {
            let content = std::fs::read_to_string(&config_path)?;
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim();
                    match key {
                        "data_dir" => data_dir = PathBuf::from(value),
                        "bind_address" => bind_address = value.to_string(),
                        "port" => {
                            port = value.parse().map_err(|_| {
                                AppError::Config(format!("Invalid port: {}", value))
                            })?;
                        }
                        _ => {}
                    }
                }
            }
        }

        // Allow environment variable overrides
        if let Ok(val) = std::env::var("SERABUTD_DATA_DIR") {
            data_dir = PathBuf::from(val);
        }
        if let Ok(val) = std::env::var("SERABUTD_BIND_ADDRESS") {
            bind_address = val;
        }
        if let Ok(val) = std::env::var("SERABUTD_PORT") {
            port = val
                .parse()
                .map_err(|_| AppError::Config(format!("Invalid SERABUTD_PORT: {}", val)))?;
        }

        Ok(Config {
            data_dir,
            bind_address,
            port,
        })
    }

    pub fn iso_dir(&self) -> PathBuf {
        self.data_dir.join("iso")
    }

    pub fn views_dir(&self) -> PathBuf {
        self.data_dir.join("views")
    }

    pub fn hardware_dir(&self) -> PathBuf {
        self.data_dir.join("hardware")
    }

    pub fn aliases_path(&self) -> PathBuf {
        self.data_dir.join("aliases.cfg")
    }

    pub fn combine_path(&self) -> PathBuf {
        self.data_dir.join("combine.cfg")
    }

    pub fn action_path(&self) -> PathBuf {
        self.data_dir.join("action.cfg")
    }
}

pub struct AppState {
    pub config: Config,
    pub aliases: AliasesConfig,
    pub combine: CombineConfig,
    pub action: RwLock<ActionConfig>,
    pub hardware: HardwareConfig,
}

impl AppState {
    pub fn new(config: Config) -> Result<Self, AppError> {
        let aliases = AliasesConfig::load(&config.aliases_path())?;
        let combine = CombineConfig::load(&config.combine_path())?;
        let action = ActionConfig::load(&config.action_path())?;
        let hardware = HardwareConfig::load(&config.hardware_dir())?;

        Ok(AppState {
            config,
            aliases,
            combine,
            action: RwLock::new(action),
            hardware,
        })
    }

    /// Derive OS family from release name
    pub fn derive_os(release: &str) -> &'static str {
        match release.split('-').next() {
            Some("debian") | Some("ubuntu") | Some("rocky") | Some("alma") | Some("centos") => {
                "linux"
            }
            Some("freebsd") | Some("openbsd") | Some("netbsd") => "bsd",
            _ => "unknown",
        }
    }

    /// Derive distro from release name
    pub fn derive_distro(release: &str) -> &'static str {
        match release.split('-').next() {
            Some("debian") => "debian",
            Some("ubuntu") => "ubuntu",
            Some("rocky") => "rocky",
            Some("alma") => "alma",
            Some("centos") => "centos",
            Some("freebsd") => "freebsd",
            Some("openbsd") => "openbsd",
            Some("netbsd") => "netbsd",
            _ => "unknown",
        }
    }

    /// Build template context for rendering
    pub fn build_template_context(
        &self,
        hostname: &str,
        host: &str,
        port: u16,
    ) -> Result<HashMap<String, String>, AppError> {
        let mut ctx = HashMap::new();

        // Server info
        ctx.insert("host".to_string(), host.to_string());
        ctx.insert("port".to_string(), port.to_string());
        ctx.insert("hostname".to_string(), hostname.to_string());

        // Hardware config
        if let Some(hw) = self.hardware.get(hostname) {
            for (k, v) in hw {
                ctx.insert(k.clone(), v.clone());
            }
        }

        // Action config
        let action = self.action.read().map_err(|_| {
            AppError::Config("Failed to read action config".to_string())
        })?;

        if let Some((release, automation)) = action.get(hostname) {
            ctx.insert("release".to_string(), release.clone());
            ctx.insert("automation".to_string(), automation.clone());
            ctx.insert("os".to_string(), Self::derive_os(&release).to_string());
            ctx.insert("distro".to_string(), Self::derive_distro(&release).to_string());
        }

        Ok(ctx)
    }
}
