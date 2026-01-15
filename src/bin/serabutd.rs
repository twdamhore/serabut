use anyhow::{Context, Result};
use clap::Parser;
use dhcproto::v4::{DhcpOption, HType, Message, MessageType, Opcode};
use dhcproto::Decodable;
use pnet::datalink::{self, Channel::Ethernet, NetworkInterface};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket};
use pnet::packet::ipv4::Ipv4Packet;
use pnet::packet::udp::UdpPacket;
use pnet::packet::Packet;
use serabut::{ensure_data_dir, read_mac_entries, update_or_insert_mac, write_mac_entries};

const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;

#[derive(Parser)]
#[command(name = "serabutd")]
#[command(about = "Serabut daemon - listens for PXE boot requests")]
struct Args {
    /// Network interface to listen on (e.g., eth0)
    #[arg(short, long)]
    interface: Option<String>,
}

fn format_mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":")
}

/// Check if vendor class indicates a PXE client
fn is_pxe_client(msg: &Message) -> bool {
    msg.opts()
        .get(dhcproto::v4::OptionCode::ClassIdentifier)
        .and_then(|opt| {
            if let DhcpOption::ClassIdentifier(class_id) = opt {
                String::from_utf8_lossy(class_id)
                    .starts_with("PXEClient")
                    .then_some(true)
            } else {
                None
            }
        })
        .unwrap_or(false)
}

/// Check if user class indicates iPXE
#[allow(dead_code)]
fn is_ipxe_client(msg: &Message) -> bool {
    msg.opts()
        .get(dhcproto::v4::OptionCode::UserClass)
        .and_then(|opt| {
            if let DhcpOption::UserClass(user_class) = opt {
                String::from_utf8_lossy(user_class)
                    .contains("iPXE")
                    .then_some(true)
            } else {
                None
            }
        })
        .unwrap_or(false)
}

fn handle_dhcp_packet(dhcp_data: &[u8]) -> Option<String> {
    // Parse DHCP message using dhcproto
    let msg = Message::decode(&mut dhcproto::decoder::Decoder::new(dhcp_data)).ok()?;

    // Only process BOOTREQUEST (client -> server)
    if msg.opcode() != Opcode::BootRequest {
        return None;
    }

    // Only process ethernet (htype=Ethernet, hlen=6)
    if msg.htype() != HType::Eth || msg.hlen() != 6 {
        return None;
    }

    // Only process DHCP DISCOVER
    let msg_type = msg.opts().msg_type()?;
    if msg_type != MessageType::Discover {
        return None;
    }

    // Only process PXE clients
    if !is_pxe_client(&msg) {
        return None;
    }

    // Extract MAC address from chaddr field
    let mac = format_mac(&msg.chaddr()[..6]);
    Some(mac)
}

fn process_packet(ethernet: &EthernetPacket) -> Option<String> {
    match ethernet.get_ethertype() {
        EtherTypes::Ipv4 => {
            if let Some(ipv4) = Ipv4Packet::new(ethernet.payload()) {
                // Check if it's UDP
                if ipv4.get_next_level_protocol()
                    == pnet::packet::ip::IpNextHeaderProtocols::Udp
                {
                    if let Some(udp) = UdpPacket::new(ipv4.payload()) {
                        // Check for DHCP (client port 68 -> server port 67)
                        if udp.get_source() == DHCP_CLIENT_PORT
                            && udp.get_destination() == DHCP_SERVER_PORT
                        {
                            return handle_dhcp_packet(udp.payload());
                        }
                    }
                }
            }
        }
        _ => {}
    }
    None
}

fn find_default_interface() -> Option<NetworkInterface> {
    let interfaces = datalink::interfaces();

    // Prefer interfaces that are up, not loopback, and have an IP
    interfaces
        .into_iter()
        .find(|iface| iface.is_up() && !iface.is_loopback() && !iface.ips.is_empty())
}

fn run_listener(interface_name: Option<&str>) -> Result<()> {
    let interface = if let Some(name) = interface_name {
        datalink::interfaces()
            .into_iter()
            .find(|iface| iface.name == name)
            .ok_or_else(|| anyhow::anyhow!("Interface '{}' not found", name))?
    } else {
        find_default_interface()
            .ok_or_else(|| anyhow::anyhow!("No suitable network interface found"))?
    };

    eprintln!("serabutd starting on interface: {}", interface.name);
    eprintln!("Listening for PXE boot requests...");

    ensure_data_dir()?;

    let (_, mut rx) = match datalink::channel(&interface, Default::default()) {
        Ok(Ethernet(_tx, rx)) => ((), rx),
        Ok(_) => return Err(anyhow::anyhow!("Unhandled channel type")),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to create datalink channel: {}. Try running as root or with CAP_NET_RAW.",
                e
            ))
        }
    };

    loop {
        match rx.next() {
            Ok(packet) => {
                if let Some(ethernet) = EthernetPacket::new(packet) {
                    if let Some(mac) = process_packet(&ethernet) {
                        eprintln!("PXE boot request from: {}", mac);

                        // Update mac.txt
                        match read_mac_entries() {
                            Ok(mut entries) => {
                                update_or_insert_mac(&mut entries, &mac);
                                if let Err(e) = write_mac_entries(&entries) {
                                    eprintln!("Failed to write mac.txt: {}", e);
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to read mac.txt: {}", e);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to read packet: {}", e);
            }
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    run_listener(args.interface.as_deref()).context("Failed to run listener")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dhcproto::v4::DhcpOptions;
    use dhcproto::Encodable;

    /// Helper to create a DHCP packet using dhcproto
    /// Note: htype and hlen parameters are for testing edge cases;
    /// for valid packets use HType::Eth and hlen=6
    fn create_dhcp_packet(
        opcode: Opcode,
        htype: HType,
        mac: [u8; 6],
        message_type: Option<MessageType>,
        vendor_class: Option<&str>,
        user_class: Option<&str>,
    ) -> Vec<u8> {
        let mut msg = Message::default();
        msg.set_opcode(opcode).set_htype(htype).set_chaddr(&mac);

        let mut opts = DhcpOptions::new();
        if let Some(mt) = message_type {
            opts.insert(DhcpOption::MessageType(mt));
        }
        if let Some(vc) = vendor_class {
            opts.insert(DhcpOption::ClassIdentifier(vc.as_bytes().to_vec()));
        }
        if let Some(uc) = user_class {
            opts.insert(DhcpOption::UserClass(uc.as_bytes().to_vec()));
        }
        msg.set_opts(opts);

        let mut buf = Vec::new();
        msg.encode(&mut dhcproto::encoder::Encoder::new(&mut buf))
            .unwrap();
        buf
    }

    mod format_mac_tests {
        use super::*;

        #[test]
        fn formats_mac_correctly() {
            let mac = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
            assert_eq!(format_mac(&mac), "aa:bb:cc:dd:ee:ff");
        }

        #[test]
        fn formats_mac_with_zeros() {
            let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
            assert_eq!(format_mac(&mac), "00:11:22:33:44:55");
        }
    }

    mod is_pxe_client_tests {
        use super::*;

        fn create_msg_with_vendor_class(vendor_class: Option<&str>) -> Message {
            let mut msg = Message::default();
            if let Some(vc) = vendor_class {
                let mut opts = DhcpOptions::new();
                opts.insert(DhcpOption::ClassIdentifier(vc.as_bytes().to_vec()));
                msg.set_opts(opts);
            }
            msg
        }

        #[test]
        fn detects_pxe_client() {
            let msg = create_msg_with_vendor_class(Some("PXEClient:Arch:00007:UNDI:003016"));
            assert!(is_pxe_client(&msg));
        }

        #[test]
        fn rejects_non_pxe() {
            let msg = create_msg_with_vendor_class(Some("MSFT 5.0"));
            assert!(!is_pxe_client(&msg));
        }

        #[test]
        fn rejects_none() {
            let msg = create_msg_with_vendor_class(None);
            assert!(!is_pxe_client(&msg));
        }
    }

    mod is_ipxe_client_tests {
        use super::*;

        fn create_msg_with_user_class(user_class: Option<&str>) -> Message {
            let mut msg = Message::default();
            if let Some(uc) = user_class {
                let mut opts = DhcpOptions::new();
                opts.insert(DhcpOption::UserClass(uc.as_bytes().to_vec()));
                msg.set_opts(opts);
            }
            msg
        }

        #[test]
        fn detects_ipxe() {
            let msg = create_msg_with_user_class(Some("iPXE"));
            assert!(is_ipxe_client(&msg));
        }

        #[test]
        fn detects_ipxe_in_longer_string() {
            let msg = create_msg_with_user_class(Some("iPXE/1.0.0"));
            assert!(is_ipxe_client(&msg));
        }

        #[test]
        fn rejects_non_ipxe() {
            let msg = create_msg_with_user_class(Some("PXEClient"));
            assert!(!is_ipxe_client(&msg));
        }

        #[test]
        fn rejects_none() {
            let msg = create_msg_with_user_class(None);
            assert!(!is_ipxe_client(&msg));
        }
    }

    mod handle_dhcp_packet_tests {
        use super::*;

        #[test]
        fn accepts_pxe_discover() {
            let packet = create_dhcp_packet(
                Opcode::BootRequest,
                HType::Eth, // Ethernet
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(MessageType::Discover),
                Some("PXEClient:Arch:00007"),
                None,
            );
            let result = handle_dhcp_packet(&packet);
            assert_eq!(result, Some("aa:bb:cc:dd:ee:ff".to_string()));
        }

        #[test]
        fn rejects_non_pxe_discover() {
            let packet = create_dhcp_packet(
                Opcode::BootRequest,
                HType::Eth,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(MessageType::Discover),
                Some("MSFT 5.0"), // Not PXE
                None,
            );
            let result = handle_dhcp_packet(&packet);
            assert!(result.is_none());
        }

        #[test]
        fn rejects_dhcp_request() {
            let packet = create_dhcp_packet(
                Opcode::BootRequest,
                HType::Eth,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(MessageType::Request), // Not DISCOVER
                Some("PXEClient:Arch:00007"),
                None,
            );
            let result = handle_dhcp_packet(&packet);
            assert!(result.is_none());
        }

        #[test]
        fn rejects_reply_packets() {
            let packet = create_dhcp_packet(
                Opcode::BootReply, // BOOTREPLY, not BOOTREQUEST
                HType::Eth,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(MessageType::Discover),
                Some("PXEClient:Arch:00007"),
                None,
            );
            let result = handle_dhcp_packet(&packet);
            assert!(result.is_none());
        }

        #[test]
        fn rejects_non_ethernet() {
            let packet = create_dhcp_packet(
                Opcode::BootRequest,
                HType::Unknown(6), // Not ethernet (6 = IEEE 802)
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(MessageType::Discover),
                Some("PXEClient:Arch:00007"),
                None,
            );
            let result = handle_dhcp_packet(&packet);
            assert!(result.is_none());
        }

        #[test]
        fn rejects_short_packet() {
            let packet = vec![0u8; 100];
            let result = handle_dhcp_packet(&packet);
            assert!(result.is_none());
        }
    }

    mod dhcp_message_type_constants {
        use super::*;

        #[test]
        fn discover_is_one() {
            // RFC 2132 defines DHCPDISCOVER as 1
            assert_eq!(u8::from(MessageType::Discover), 1);
        }

        #[test]
        fn offer_is_two() {
            // RFC 2132 defines DHCPOFFER as 2
            assert_eq!(u8::from(MessageType::Offer), 2);
        }

        #[test]
        fn request_is_three() {
            // RFC 2132 defines DHCPREQUEST as 3
            assert_eq!(u8::from(MessageType::Request), 3);
        }
    }
}
