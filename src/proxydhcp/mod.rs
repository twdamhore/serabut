//! ProxyDHCP server module.
//!
//! Implements a proxyDHCP server that provides PXE boot information
//! without interfering with the main DHCP server's IP allocation.

mod server;

pub use server::ProxyDhcpServer;
