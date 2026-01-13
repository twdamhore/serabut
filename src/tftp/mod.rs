//! TFTP server module.
//!
//! Implements a simple TFTP server for serving PXE boot files.

mod server;

pub use server::TftpServer;
