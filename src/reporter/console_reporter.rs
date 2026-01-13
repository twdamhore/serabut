//! Console-based event reporter.

use std::io::{self, Write};

use crate::domain::{DhcpMessageType, PxeBootEvent};
use crate::reporter::EventReporter;

/// Reports PXE boot events to the console.
///
/// Formats events in a human-readable format suitable for
/// terminal output.
pub struct ConsoleReporter {
    /// Whether to use colors in output
    use_colors: bool,
    /// Whether to show verbose output
    verbose: bool,
}

impl ConsoleReporter {
    /// Create a new console reporter.
    pub fn new() -> Self {
        Self {
            use_colors: true,
            verbose: false,
        }
    }

    /// Enable or disable colored output.
    pub fn with_colors(mut self, use_colors: bool) -> Self {
        self.use_colors = use_colors;
        self
    }

    /// Enable or disable verbose output.
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    fn format_event(&self, event: &PxeBootEvent) -> String {
        let mac = event.client_mac;
        let msg_type = &event.message_type;
        let xid = event.transaction_id;

        let mut output = String::new();

        // Format based on message type
        match msg_type {
            DhcpMessageType::Discover => {
                output.push_str(&format!(
                    "[PXE DISCOVER] MAC: {} | XID: {:#010x}",
                    mac, xid
                ));
            }
            DhcpMessageType::Request => {
                output.push_str(&format!(
                    "[PXE REQUEST]  MAC: {} | XID: {:#010x}",
                    mac, xid
                ));
            }
            DhcpMessageType::Offer => {
                let ip = event
                    .assigned_ip
                    .map(|ip| ip.to_string())
                    .unwrap_or_else(|| "N/A".to_string());
                let server = event
                    .server_ip
                    .map(|ip| ip.to_string())
                    .unwrap_or_else(|| "N/A".to_string());

                output.push_str(&format!(
                    "[PXE OFFER]    MAC: {} | IP: {} | Server: {} | XID: {:#010x}",
                    mac, ip, server, xid
                ));
            }
            DhcpMessageType::Ack => {
                let ip = event
                    .assigned_ip
                    .map(|ip| ip.to_string())
                    .unwrap_or_else(|| "N/A".to_string());
                let server = event
                    .server_ip
                    .map(|ip| ip.to_string())
                    .unwrap_or_else(|| "N/A".to_string());

                output.push_str(&format!(
                    "[PXE ACK]      MAC: {} | IP: {} | Server: {} | XID: {:#010x}",
                    mac, ip, server, xid
                ));
            }
            _ => {
                output.push_str(&format!("[PXE {}] MAC: {} | XID: {:#010x}", msg_type, mac, xid));
            }
        }

        // Add architecture info if verbose or always show for key events
        if let Some(arch) = &event.pxe_info.architecture {
            output.push_str(&format!(" | Arch: {}", arch));
        }

        // Add UUID if present and verbose
        if self.verbose {
            if let Some(uuid) = &event.pxe_info.uuid {
                output.push_str(&format!(" | UUID: {}", uuid));
            }
            output.push_str(&format!(" | Vendor: {}", event.pxe_info.vendor_class));
        }

        output
    }
}

impl Default for ConsoleReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl EventReporter for ConsoleReporter {
    fn report(&self, event: &PxeBootEvent) {
        let output = self.format_event(event);
        let mut stdout = io::stdout().lock();
        let _ = writeln!(stdout, "{}", output);
    }

    fn on_start(&self, interface: &str) {
        println!("Listening for PXE boot requests on interface: {}", interface);
        println!("Press Ctrl+C to stop.\n");
    }

    fn on_stop(&self) {
        println!("\nStopping PXE boot listener.");
    }
}
