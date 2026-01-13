//! pnet-based packet capture implementation.

use pnet::datalink::{self, Channel, Config, NetworkInterface};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::ipv4::Ipv4Packet;
use pnet::packet::udp::UdpPacket;
use pnet::packet::Packet;

use super::{PacketCapture, RawPacket};
use crate::error::CaptureError;

/// DHCP server port
const DHCP_SERVER_PORT: u16 = 67;
/// DHCP client port
const DHCP_CLIENT_PORT: u16 = 68;

/// Packet capture using the pnet library.
pub struct PnetCapture {
    interface: NetworkInterface,
}

impl PnetCapture {
    /// Create a new capture on the specified interface.
    pub fn new(interface_name: &str) -> Result<Self, CaptureError> {
        let interface = datalink::interfaces()
            .into_iter()
            .find(|iface| iface.name == interface_name)
            .ok_or_else(|| CaptureError::InterfaceNotFound(interface_name.to_string()))?;

        Ok(Self { interface })
    }

    /// Create a capture on the first suitable interface.
    ///
    /// Looks for an interface that is up and not a loopback.
    pub fn on_default_interface() -> Result<Self, CaptureError> {
        let interface = datalink::interfaces()
            .into_iter()
            .find(|iface| iface.is_up() && !iface.is_loopback() && !iface.ips.is_empty())
            .ok_or_else(|| {
                CaptureError::InterfaceNotFound("no suitable interface found".to_string())
            })?;

        Ok(Self { interface })
    }

    /// List all available network interfaces.
    pub fn list_interfaces() -> Vec<String> {
        datalink::interfaces()
            .into_iter()
            .map(|iface| {
                let status = if iface.is_up() { "UP" } else { "DOWN" };
                let ips: Vec<_> = iface.ips.iter().map(|ip| ip.to_string()).collect();
                format!(
                    "{}: {} [{}]",
                    iface.name,
                    status,
                    if ips.is_empty() {
                        "no IP".to_string()
                    } else {
                        ips.join(", ")
                    }
                )
            })
            .collect()
    }
}

impl PacketCapture for PnetCapture {
    fn capture_dhcp_packets(
        &mut self,
    ) -> Result<Box<dyn Iterator<Item = RawPacket> + '_>, CaptureError> {
        let config = Config {
            read_timeout: Some(std::time::Duration::from_millis(100)),
            ..Config::default()
        };

        let (_tx, rx) = match datalink::channel(&self.interface, config) {
            Ok(Channel::Ethernet(tx, rx)) => (tx, rx),
            Ok(_) => {
                return Err(CaptureError::ChannelCreation(
                    "unsupported channel type".to_string(),
                ))
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("permission") || msg.contains("Operation not permitted") {
                    return Err(CaptureError::InsufficientPermissions);
                }
                return Err(CaptureError::ChannelCreation(msg));
            }
        };

        Ok(Box::new(DhcpPacketIterator { rx }))
    }

    fn interface_name(&self) -> &str {
        &self.interface.name
    }
}

/// Iterator that yields DHCP packets from the network.
struct DhcpPacketIterator {
    rx: Box<dyn datalink::DataLinkReceiver>,
}

impl Iterator for DhcpPacketIterator {
    type Item = RawPacket;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.rx.next() {
                Ok(packet) => {
                    if let Some(dhcp_packet) = extract_dhcp_packet(packet) {
                        return Some(dhcp_packet);
                    }
                    // Not a DHCP packet, continue listening
                }
                Err(e) => {
                    // Timeout is expected, continue
                    if e.kind() == std::io::ErrorKind::TimedOut {
                        continue;
                    }
                    // For other errors, log and continue
                    tracing::debug!("Capture error: {}", e);
                    continue;
                }
            }
        }
    }
}

/// Extract DHCP payload from an Ethernet frame if it's a DHCP packet.
fn extract_dhcp_packet(data: &[u8]) -> Option<RawPacket> {
    let ethernet = EthernetPacket::new(data)?;

    // We only care about IPv4
    if ethernet.get_ethertype() != EtherTypes::Ipv4 {
        return None;
    }

    let ipv4 = Ipv4Packet::new(ethernet.payload())?;

    // We only care about UDP
    if ipv4.get_next_level_protocol() != IpNextHeaderProtocols::Udp {
        return None;
    }

    let udp = UdpPacket::new(ipv4.payload())?;

    // Check if it's DHCP (ports 67 or 68)
    let src_port = udp.get_source();
    let dst_port = udp.get_destination();

    if !is_dhcp_port(src_port) && !is_dhcp_port(dst_port) {
        return None;
    }

    let src_mac = ethernet.get_source().octets();
    let dst_mac = ethernet.get_destination().octets();

    Some(RawPacket {
        data: udp.payload().to_vec(),
        src_mac,
        dst_mac,
    })
}

fn is_dhcp_port(port: u16) -> bool {
    port == DHCP_SERVER_PORT || port == DHCP_CLIENT_PORT
}
