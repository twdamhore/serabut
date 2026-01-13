//! Netboot image management module.
//!
//! Handles downloading, verifying, and extracting netboot images
//! for various operating systems.

mod autoinstall;
mod config;
mod manager;

pub use autoinstall::{AutoinstallConfig, BootloaderConfigGenerator};
pub use config::{NetbootArch, NetbootConfig, NetbootConfigs};
pub use manager::NetbootManager;
