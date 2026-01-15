use anyhow::{Context, Result};
use clap::Parser;
use pnet::datalink::{self, Channel::Ethernet, NetworkInterface};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket};
use pnet::packet::ipv4::Ipv4Packet;
use pnet::packet::udp::UdpPacket;
use pnet::packet::Packet;
use serabut::{ensure_data_dir, read_mac_entries, update_or_insert_mac, write_mac_entries};

const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;

// DHCP message types
const DHCP_DISCOVER: u8 = 1;
#[allow(dead_code)]
const DHCP_OFFER: u8 = 2;
#[allow(dead_code)]
const DHCP_REQUEST: u8 = 3;

// DHCP options
const DHCP_OPTION_MESSAGE_TYPE: u8 = 53;
const DHCP_OPTION_VENDOR_CLASS: u8 = 60;
#[allow(dead_code)]
const DHCP_OPTION_USER_CLASS: u8 = 77; // Used to detect iPXE vs PXE ROM
const DHCP_OPTION_END: u8 = 255;

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

fn parse_dhcp_options(data: &[u8]) -> (Option<u8>, Option<String>, Option<String>) {
    let mut message_type = None;
    let mut vendor_class = None;
    let mut user_class = None;

    // DHCP options start at offset 240 (after magic cookie)
    if data.len() < 240 {
        return (message_type, vendor_class, user_class);
    }

    // Check magic cookie (99, 130, 83, 99)
    if data[236..240] != [99, 130, 83, 99] {
        return (message_type, vendor_class, user_class);
    }

    let mut i = 240;
    while i < data.len() {
        let option = data[i];
        if option == DHCP_OPTION_END {
            break;
        }
        if option == 0 {
            // Padding
            i += 1;
            continue;
        }
        if i + 1 >= data.len() {
            break;
        }
        let len = data[i + 1] as usize;
        if i + 2 + len > data.len() {
            break;
        }
        let value = &data[i + 2..i + 2 + len];

        match option {
            DHCP_OPTION_MESSAGE_TYPE => {
                if len >= 1 {
                    message_type = Some(value[0]);
                }
            }
            DHCP_OPTION_VENDOR_CLASS => {
                vendor_class = Some(String::from_utf8_lossy(value).to_string());
            }
            DHCP_OPTION_USER_CLASS => {
                user_class = Some(String::from_utf8_lossy(value).to_string());
            }
            _ => {}
        }

        i += 2 + len;
    }

    (message_type, vendor_class, user_class)
}

fn is_pxe_request(vendor_class: &Option<String>) -> bool {
    vendor_class
        .as_ref()
        .map(|vc| vc.starts_with("PXEClient"))
        .unwrap_or(false)
}

#[allow(dead_code)]
fn is_ipxe_request(user_class: &Option<String>) -> bool {
    user_class
        .as_ref()
        .map(|uc| uc.contains("iPXE"))
        .unwrap_or(false)
}

fn handle_dhcp_packet(dhcp_data: &[u8]) -> Option<String> {
    // DHCP packet structure:
    // 0: op (1 = request, 2 = reply)
    // 1: htype (1 = ethernet)
    // 2: hlen (6 for ethernet)
    // 3: hops
    // 4-7: xid
    // 8-9: secs
    // 10-11: flags
    // 12-15: ciaddr
    // 16-19: yiaddr
    // 20-23: siaddr
    // 24-27: giaddr
    // 28-43: chaddr (client hardware address, 16 bytes, only first 6 used for ethernet)

    if dhcp_data.len() < 240 {
        return None;
    }

    let op = dhcp_data[0];
    if op != 1 {
        // Not a request
        return None;
    }

    let htype = dhcp_data[1];
    let hlen = dhcp_data[2];
    if htype != 1 || hlen != 6 {
        // Not ethernet
        return None;
    }

    let mac = format_mac(&dhcp_data[28..34]);
    let (message_type, vendor_class, _user_class) = parse_dhcp_options(dhcp_data);

    // Only process DHCP DISCOVER with PXE vendor class
    if message_type != Some(DHCP_DISCOVER) {
        return None;
    }

    if !is_pxe_request(&vendor_class) {
        return None;
    }

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
