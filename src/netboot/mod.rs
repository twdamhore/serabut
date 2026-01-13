//! Netboot image management module.
//!
//! Handles downloading, verifying, and extracting netboot images
//! for various operating systems.

mod config;
mod manager;

pub use config::{NetbootArch, NetbootConfig, NetbootConfigs};
pub use manager::NetbootManager;
