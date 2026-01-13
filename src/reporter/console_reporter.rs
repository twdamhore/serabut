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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PxeInfo;
    use macaddr::MacAddr6;
    use std::net::Ipv4Addr;

    fn create_pxe_info() -> PxeInfo {
        PxeInfo::from_vendor_class("PXEClient:Arch:00007:UNDI:003016").unwrap()
    }

    fn create_pxe_info_with_uuid() -> PxeInfo {
        PxeInfo::from_vendor_class("PXEClient:Arch:00007:UNDI:003016")
            .unwrap()
            .with_uuid(&[
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
                0x0d, 0x0e, 0x0f, 0x10,
            ])
    }

    #[test]
    fn test_new() {
        let reporter = ConsoleReporter::new();
        assert!(reporter.use_colors);
        assert!(!reporter.verbose);
    }

    #[test]
    fn test_default() {
        let reporter = ConsoleReporter::default();
        assert!(reporter.use_colors);
        assert!(!reporter.verbose);
    }

    #[test]
    fn test_with_colors() {
        let reporter = ConsoleReporter::new().with_colors(false);
        assert!(!reporter.use_colors);
    }

    #[test]
    fn test_with_verbose() {
        let reporter = ConsoleReporter::new().with_verbose(true);
        assert!(reporter.verbose);
    }

    #[test]
    fn test_format_discover() {
        let reporter = ConsoleReporter::new();
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event = PxeBootEvent::from_request(
            mac,
            0x12345678,
            DhcpMessageType::Discover,
            create_pxe_info(),
        );

        let output = reporter.format_event(&event);
        assert!(output.contains("[PXE DISCOVER]"));
        assert!(output.contains("AA:BB:CC:DD:EE:FF"));
        assert!(output.contains("0x12345678"));
    }

    #[test]
    fn test_format_request() {
        let reporter = ConsoleReporter::new();
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event = PxeBootEvent::from_request(
            mac,
            0x12345678,
            DhcpMessageType::Request,
            create_pxe_info(),
        );

        let output = reporter.format_event(&event);
        assert!(output.contains("[PXE REQUEST]"));
    }

    #[test]
    fn test_format_offer() {
        let reporter = ConsoleReporter::new();
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event = PxeBootEvent::from_reply(
            mac,
            0x12345678,
            DhcpMessageType::Offer,
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 1),
            create_pxe_info(),
        );

        let output = reporter.format_event(&event);
        assert!(output.contains("[PXE OFFER]"));
        assert!(output.contains("192.168.1.100"));
        assert!(output.contains("192.168.1.1"));
    }

    #[test]
    fn test_format_ack() {
        let reporter = ConsoleReporter::new();
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event = PxeBootEvent::from_reply(
            mac,
            0x12345678,
            DhcpMessageType::Ack,
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 1),
            create_pxe_info(),
        );

        let output = reporter.format_event(&event);
        assert!(output.contains("[PXE ACK]"));
        assert!(output.contains("192.168.1.100"));
    }

    #[test]
    fn test_format_other_message_type() {
        let reporter = ConsoleReporter::new();
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event = PxeBootEvent::from_request(
            mac,
            0x12345678,
            DhcpMessageType::Release,
            create_pxe_info(),
        );

        let output = reporter.format_event(&event);
        assert!(output.contains("[PXE RELEASE]"));
    }

    #[test]
    fn test_format_with_architecture() {
        let reporter = ConsoleReporter::new();
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event = PxeBootEvent::from_request(
            mac,
            0x12345678,
            DhcpMessageType::Discover,
            create_pxe_info(),
        );

        let output = reporter.format_event(&event);
        assert!(output.contains("Arch: EFI x64"));
    }

    #[test]
    fn test_format_verbose_with_uuid() {
        let reporter = ConsoleReporter::new().with_verbose(true);
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event = PxeBootEvent::from_request(
            mac,
            0x12345678,
            DhcpMessageType::Discover,
            create_pxe_info_with_uuid(),
        );

        let output = reporter.format_event(&event);
        assert!(output.contains("UUID:"));
        assert!(output.contains("Vendor: PXEClient"));
    }

    #[test]
    fn test_format_verbose_without_uuid() {
        let reporter = ConsoleReporter::new().with_verbose(true);
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event = PxeBootEvent::from_request(
            mac,
            0x12345678,
            DhcpMessageType::Discover,
            create_pxe_info(),
        );

        let output = reporter.format_event(&event);
        assert!(!output.contains("UUID:"));
        assert!(output.contains("Vendor: PXEClient"));
    }

    #[test]
    fn test_format_offer_without_ips() {
        let reporter = ConsoleReporter::new();
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);

        // Create event without IPs (simulating edge case)
        let mut event = PxeBootEvent::from_request(
            mac,
            0x12345678,
            DhcpMessageType::Offer,
            create_pxe_info(),
        );
        event.message_type = DhcpMessageType::Offer;
        // assigned_ip and server_ip are None from from_request

        let output = reporter.format_event(&event);
        assert!(output.contains("IP: N/A"));
        assert!(output.contains("Server: N/A"));
    }

    #[test]
    fn test_format_without_architecture() {
        let reporter = ConsoleReporter::new();
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);

        // Create PxeInfo without architecture
        let pxe_info = PxeInfo::from_vendor_class("PXEClient").unwrap();
        let event = PxeBootEvent::from_request(
            mac,
            0x12345678,
            DhcpMessageType::Discover,
            pxe_info,
        );

        let output = reporter.format_event(&event);
        assert!(!output.contains("Arch:"));
    }
}
