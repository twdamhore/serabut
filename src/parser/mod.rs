//! DHCP packet parsing module.
//!
//! This module is responsible for parsing raw bytes into domain DHCP types (SRP).

mod dhcp_parser;

pub use dhcp_parser::DhcpParser;
