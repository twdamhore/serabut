//! ProxyDHCP server implementation.
//!
//! Listens for PXE boot requests and responds with boot server information.
//! Works alongside the existing DHCP server without providing IP addresses.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use macaddr::MacAddr6;
use tracing::{error, info};

use crate::domain::{DhcpMessageType, PxeClientArch};
use crate::parser::DhcpParser;

/// DHCP ports
const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;

/// ProxyDHCP port (for directed requests)
const PROXY_DHCP_PORT: u16 = 4011;

/// DHCP option codes
const OPTION_DHCP_MESSAGE_TYPE: u8 = 53;
const OPTION_SERVER_IDENTIFIER: u8 = 54;
const OPTION_VENDOR_CLASS_ID: u8 = 60;
const OPTION_CLIENT_ARCH: u8 = 93;
const OPTION_CLIENT_NDI: u8 = 94;
const OPTION_CLIENT_UUID: u8 = 97;
const OPTION_PXE_MENU: u8 = 43;  // Vendor-specific (encapsulated)
const OPTION_END: u8 = 255;

/// DHCP magic cookie
const DHCP_MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];

/// ProxyDHCP server for PXE boot.
pub struct ProxyDhcpServer {
    /// Our server IP address.
    server_ip: Ipv4Addr,
    /// BIOS boot filename.
    boot_file_bios: String,
    /// EFI boot filename.
    boot_file_efi: String,
    /// Running flag.
    running: Arc<AtomicBool>,
}

impl ProxyDhcpServer {
    /// Create a new proxyDHCP server.
    ///
    /// # Arguments
    /// * `server_ip` - Our IP address (TFTP server)
    /// * `boot_file_bios` - Boot filename for BIOS clients (e.g., "pxelinux.0")
    /// * `boot_file_efi` - Boot filename for EFI clients (e.g., "grubnetx64.efi.signed")
    pub fn new(
        server_ip: Ipv4Addr,
        boot_file_bios: impl Into<String>,
        boot_file_efi: impl Into<String>,
    ) -> Self {
        Self {
            server_ip,
            boot_file_bios: boot_file_bios.into(),
            boot_file_efi: boot_file_efi.into(),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a handle to stop the server.
    pub fn running_flag(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// Start the proxyDHCP server.
    ///
    /// Listens on ports 67 and 4011 for PXE boot requests.
    pub fn run(&self) -> Result<()> {
        // Bind to port 67 for broadcast DHCP
        // We need to be able to receive broadcasts
        let socket67 = self.create_socket(DHCP_SERVER_PORT)?;

        // Also bind to port 4011 for direct proxyDHCP requests
        let socket4011 = self.create_socket(PROXY_DHCP_PORT)?;

        info!("ProxyDHCP server listening on ports {} and {}", DHCP_SERVER_PORT, PROXY_DHCP_PORT);
        info!("Server IP: {}", self.server_ip);
        info!("BIOS boot file: {}", self.boot_file_bios);
        info!("EFI boot file: {}", self.boot_file_efi);

        self.running.store(true, Ordering::SeqCst);

        let mut buf = [0u8; 1500];

        while self.running.load(Ordering::SeqCst) {
            // Check both sockets with timeout
            if let Ok((len, addr)) = socket67.recv_from(&mut buf) {
                if len >= 240 {
                    self.handle_packet(&socket67, &buf[..len], addr);
                }
            }

            if let Ok((len, addr)) = socket4011.recv_from(&mut buf) {
                if len >= 240 {
                    self.handle_packet(&socket4011, &buf[..len], addr);
                }
            }
        }

        info!("ProxyDHCP server stopped");
        Ok(())
    }

    /// Create a UDP socket with broadcast enabled.
    fn create_socket(&self, port: u16) -> Result<UdpSocket> {
        use socket2::{Domain, Protocol, Socket, Type};

        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .context("Failed to create socket")?;

        socket.set_reuse_address(true)?;
        socket.set_broadcast(true)?;

        let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
        socket.bind(&addr.into())
            .with_context(|| format!("Failed to bind to port {}", port))?;

        socket.set_read_timeout(Some(Duration::from_millis(100)))?;

        Ok(socket.into())
    }

    /// Handle an incoming DHCP packet.
    fn handle_packet(&self, socket: &UdpSocket, data: &[u8], from: SocketAddr) {
        // Quick sanity check
        if data.len() < 240 {
            return;
        }

        // Check op code (should be BOOTREQUEST = 1)
        if data[0] != 1 {
            return;
        }

        // Parse the DHCP packet
        let parser = DhcpParser::new();
        let packet = match parser.parse(data) {
            Ok(p) => p,
            Err(_) => return,
        };

        // Check if this is a PXE client
        let vendor_class = match packet.vendor_class_id() {
            Some(vc) if vc.starts_with("PXEClient") => vc,
            _ => return, // Not a PXE client
        };

        // Get message type
        let msg_type = match packet.message_type() {
            Some(t) => t,
            None => return,
        };

        // We only respond to DISCOVER and REQUEST
        match msg_type {
            DhcpMessageType::Discover => {
                info!(
                    "PXE DISCOVER from {} (XID: 0x{:08X})",
                    format_mac(packet.chaddr),
                    packet.xid
                );
                self.send_offer(socket, data, &vendor_class);
            }
            DhcpMessageType::Request => {
                // Check if this is a request to us (port 4011) or broadcast
                let from_port = match from {
                    SocketAddr::V4(addr) => addr.port(),
                    _ => 0,
                };

                // Only respond if directed to us or if we're the server identifier
                if from_port == DHCP_CLIENT_PORT {
                    info!(
                        "PXE REQUEST from {} (XID: 0x{:08X})",
                        format_mac(packet.chaddr),
                        packet.xid
                    );
                    self.send_ack(socket, data, &vendor_class);
                }
            }
            _ => {}
        }
    }

    /// Send a DHCP OFFER with PXE boot information.
    fn send_offer(&self, socket: &UdpSocket, request: &[u8], vendor_class: &str) {
        if let Some(response) = self.build_response(request, DhcpMessageType::Offer, vendor_class) {
            let dest = SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::BROADCAST,
                DHCP_CLIENT_PORT,
            ));

            match socket.send_to(&response, dest) {
                Ok(_) => {
                    let mac = extract_mac(request);
                    info!(
                        "PXE OFFER sent to {} -> boot file: {}",
                        format_mac(mac),
                        self.get_boot_file(vendor_class)
                    );
                }
                Err(e) => {
                    error!("Failed to send OFFER: {}", e);
                }
            }
        }
    }

    /// Send a DHCP ACK with PXE boot information.
    fn send_ack(&self, socket: &UdpSocket, request: &[u8], vendor_class: &str) {
        if let Some(response) = self.build_response(request, DhcpMessageType::Ack, vendor_class) {
            let dest = SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::BROADCAST,
                DHCP_CLIENT_PORT,
            ));

            match socket.send_to(&response, dest) {
                Ok(_) => {
                    let mac = extract_mac(request);
                    info!(
                        "PXE ACK sent to {} -> TFTP: {}",
                        format_mac(mac),
                        self.server_ip
                    );
                }
                Err(e) => {
                    error!("Failed to send ACK: {}", e);
                }
            }
        }
    }

    /// Build a DHCP response packet.
    fn build_response(
        &self,
        request: &[u8],
        msg_type: DhcpMessageType,
        vendor_class: &str,
    ) -> Option<Vec<u8>> {
        if request.len() < 240 {
            return None;
        }

        let boot_file = self.get_boot_file(vendor_class);

        // Build response (start with 576 byte minimum)
        let mut response = vec![0u8; 576];

        // Op: BOOTREPLY
        response[0] = 2;

        // Copy hardware type, len, hops
        response[1..4].copy_from_slice(&request[1..4]);

        // Transaction ID
        response[4..8].copy_from_slice(&request[4..8]);

        // Secs and flags
        response[8..12].copy_from_slice(&request[8..12]);

        // ciaddr (leave 0)
        // yiaddr (leave 0 - we're not assigning IPs)

        // siaddr - our TFTP server IP
        response[20..24].copy_from_slice(&self.server_ip.octets());

        // giaddr - copy from request
        response[24..28].copy_from_slice(&request[24..28]);

        // chaddr - client hardware address
        response[28..44].copy_from_slice(&request[28..44]);

        // sname (server host name) - leave blank
        // file (boot file name) - set to our boot file
        let file_bytes = boot_file.as_bytes();
        let copy_len = file_bytes.len().min(128);
        response[108..108 + copy_len].copy_from_slice(&file_bytes[..copy_len]);

        // Magic cookie
        response[236..240].copy_from_slice(&DHCP_MAGIC_COOKIE);

        // Options start at offset 240
        let mut opt_offset = 240;

        // Option 53: DHCP Message Type
        response[opt_offset] = OPTION_DHCP_MESSAGE_TYPE;
        response[opt_offset + 1] = 1;
        response[opt_offset + 2] = msg_type as u8;
        opt_offset += 3;

        // Option 54: Server Identifier (our IP)
        response[opt_offset] = OPTION_SERVER_IDENTIFIER;
        response[opt_offset + 1] = 4;
        response[opt_offset + 2..opt_offset + 6].copy_from_slice(&self.server_ip.octets());
        opt_offset += 6;

        // Option 60: Vendor Class ID (echo back PXEClient)
        let pxe_class = b"PXEClient";
        response[opt_offset] = OPTION_VENDOR_CLASS_ID;
        response[opt_offset + 1] = pxe_class.len() as u8;
        response[opt_offset + 2..opt_offset + 2 + pxe_class.len()].copy_from_slice(pxe_class);
        opt_offset += 2 + pxe_class.len();

        // Option 43: Vendor-specific information (PXE)
        // Sub-option 6: PXE_DISCOVERY_CONTROL = 8 (disable broadcast, use boot server)
        let pxe_vendor_opts = [
            6, 1, 8,  // PXE_DISCOVERY_CONTROL: disable broadcast, use unicast
        ];
        response[opt_offset] = OPTION_PXE_MENU;
        response[opt_offset + 1] = pxe_vendor_opts.len() as u8;
        response[opt_offset + 2..opt_offset + 2 + pxe_vendor_opts.len()]
            .copy_from_slice(&pxe_vendor_opts);
        opt_offset += 2 + pxe_vendor_opts.len();

        // Option 255: End
        response[opt_offset] = OPTION_END;
        opt_offset += 1;

        // Truncate to actual size
        response.truncate(opt_offset);

        // Pad to minimum DHCP packet size (300 bytes)
        while response.len() < 300 {
            response.push(0);
        }

        Some(response)
    }

    /// Get the appropriate boot file based on client architecture.
    fn get_boot_file(&self, vendor_class: &str) -> &str {
        // Parse architecture from vendor class
        // Format: PXEClient:Arch:00007:UNDI:003016
        if let Some(arch_str) = vendor_class.split(':').nth(2) {
            if let Ok(arch_num) = arch_str.parse::<u16>() {
                let arch = PxeClientArch::from_u16(arch_num);
                if arch.is_efi() {
                    return &self.boot_file_efi;
                }
            }
        }

        // Check for EFI in the vendor class string
        if vendor_class.contains("EFI") || vendor_class.contains("00007") {
            &self.boot_file_efi
        } else {
            &self.boot_file_bios
        }
    }
}

/// Extract MAC address from DHCP packet.
fn extract_mac(packet: &[u8]) -> MacAddr6 {
    if packet.len() >= 34 {
        MacAddr6::new(
            packet[28],
            packet[29],
            packet[30],
            packet[31],
            packet[32],
            packet[33],
        )
    } else {
        MacAddr6::nil()
    }
}

/// Format MAC address for display.
fn format_mac(mac: MacAddr6) -> String {
    format!("{}", mac).to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let server = ProxyDhcpServer::new(
            Ipv4Addr::new(192, 168, 1, 100),
            "pxelinux.0",
            "grubnetx64.efi.signed",
        );
        assert_eq!(server.server_ip, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(server.boot_file_bios, "pxelinux.0");
        assert_eq!(server.boot_file_efi, "grubnetx64.efi.signed");
    }

    #[test]
    fn test_get_boot_file_bios() {
        let server = ProxyDhcpServer::new(
            Ipv4Addr::new(192, 168, 1, 100),
            "pxelinux.0",
            "grubnetx64.efi.signed",
        );
        // BIOS architecture (00000)
        assert_eq!(
            server.get_boot_file("PXEClient:Arch:00000:UNDI:002001"),
            "pxelinux.0"
        );
    }

    #[test]
    fn test_get_boot_file_efi() {
        let server = ProxyDhcpServer::new(
            Ipv4Addr::new(192, 168, 1, 100),
            "pxelinux.0",
            "grubnetx64.efi.signed",
        );
        // EFI x64 architecture (00007)
        assert_eq!(
            server.get_boot_file("PXEClient:Arch:00007:UNDI:003016"),
            "grubnetx64.efi.signed"
        );
    }

    #[test]
    fn test_running_flag() {
        let server = ProxyDhcpServer::new(
            Ipv4Addr::new(192, 168, 1, 100),
            "pxelinux.0",
            "grubnetx64.efi.signed",
        );
        let flag = server.running_flag();
        assert!(!flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_format_mac() {
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        assert_eq!(format_mac(mac), "AA:BB:CC:DD:EE:FF");
    }
}
