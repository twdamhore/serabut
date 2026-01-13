//! Netboot image management module.
//!
//! Handles downloading, verifying, and extracting Ubuntu netboot images.

mod manager;

pub use manager::NetbootManager;
