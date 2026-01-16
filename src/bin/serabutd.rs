use anyhow::{Context, Result};
use clap::Parser;
use pnet::datalink::{self, Channel::Ethernet, DataLinkSender, NetworkInterface};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::ipv4::{Ipv4Packet, MutableIpv4Packet, checksum as ipv4_checksum};
use pnet::packet::udp::{MutableUdpPacket, UdpPacket};
use pnet::packet::Packet;
use pnet::util::MacAddr;
use serabut::{
    ensure_data_dir, find_boot_by_mac, normalize_mac, read_boot_entries, read_mac_entries,
    read_profile, update_or_insert_mac, write_boot_entries, write_mac_entries,
};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;

// DHCP message types
const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER: u8 = 2;
const DHCP_REQUEST: u8 = 3;
const DHCP_ACK: u8 = 5;

// DHCP options
const DHCP_OPTION_MESSAGE_TYPE: u8 = 53;
const DHCP_OPTION_SERVER_ID: u8 = 54;
const DHCP_OPTION_VENDOR_CLASS: u8 = 60;
const DHCP_OPTION_TFTP_SERVER: u8 = 66;
const DHCP_OPTION_BOOTFILE: u8 = 67;
const DHCP_OPTION_USER_CLASS: u8 = 77; // Used to detect iPXE vs PXE ROM
const DHCP_OPTION_IPXE_ENCAP: u8 = 175; // iPXE encapsulated options
const DHCP_OPTION_END: u8 = 255;

// iPXE sub-options within option 175
const IPXE_OPTION_SCRIPT: u8 = 8; // Boot script URL

#[derive(Parser)]
#[command(name = "serabutd")]
#[command(about = "Serabut daemon - PXE boot server with ProxyDHCP")]
struct Args {
    /// Network interface to listen on (e.g., eth0, br0)
    #[arg(short, long)]
    interface: Option<String>,

    /// HTTP port for boot scripts (default: 6007)
    #[arg(long, default_value = "6007")]
    http_port: u16,

    /// TFTP boot filename for PXE ROM clients (default: ipxe.efi)
    #[arg(long, default_value = "ipxe.efi")]
    boot_file: String,

    /// Disable sending ProxyDHCP responses (listen-only mode)
    #[arg(long)]
    no_respond: bool,
}

/// Server configuration passed around
struct ServerConfig {
    server_ip: Ipv4Addr,
    http_port: u16,
    boot_file: String,
    respond: bool,
    interface_name: String,
}

fn format_mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":")
}

/// Get the IPv4 address of an interface
fn get_interface_ip(interface: &NetworkInterface) -> Option<Ipv4Addr> {
    for ip in &interface.ips {
        if let pnet::ipnetwork::IpNetwork::V4(ipv4) = ip {
            return Some(ipv4.ip());
        }
    }
    None
}

/// Build a ProxyDHCP OFFER packet
fn build_dhcp_offer(
    request: &[u8],
    config: &ServerConfig,
    is_ipxe: bool,
) -> Vec<u8> {
    let mut response = vec![0u8; 300]; // Base size, will grow with options

    // BOOTP header
    response[0] = 2; // op: BOOTREPLY
    response[1] = 1; // htype: Ethernet
    response[2] = 6; // hlen: MAC address length
    response[3] = 0; // hops

    // Copy XID from request (bytes 4-7)
    response[4..8].copy_from_slice(&request[4..8]);

    // secs = 0 (bytes 8-9), copy flags from request (bytes 10-11)
    // The broadcast flag (0x8000) must be preserved - UEFI PXE requires this
    response[10..12].copy_from_slice(&request[10..12]);

    // ciaddr = 0 (bytes 12-15) - client doesn't have IP yet
    // yiaddr = 0 (bytes 16-19) - ProxyDHCP doesn't assign IP
    // siaddr = our IP (bytes 20-23) - TFTP server
    response[20..24].copy_from_slice(&config.server_ip.octets());

    // giaddr = 0 (bytes 24-27)

    // chaddr - copy from request (bytes 28-43)
    response[28..44].copy_from_slice(&request[28..44]);

    // sname (bytes 44-107) - server name, leave empty
    // file (bytes 108-235) - boot filename for TFTP
    if !is_ipxe {
        let boot_file_bytes = config.boot_file.as_bytes();
        let len = boot_file_bytes.len().min(127);
        response[108..108 + len].copy_from_slice(&boot_file_bytes[..len]);
    }

    // Magic cookie (bytes 236-239)
    response[236] = 99;
    response[237] = 130;
    response[238] = 83;
    response[239] = 99;

    // DHCP options start at byte 240
    let mut options = Vec::new();

    // Option 53: DHCP Message Type = OFFER
    options.push(DHCP_OPTION_MESSAGE_TYPE);
    options.push(1);
    options.push(DHCP_OFFER);

    // Option 54: Server Identifier
    options.push(DHCP_OPTION_SERVER_ID);
    options.push(4);
    options.extend_from_slice(&config.server_ip.octets());

    // Option 60: Vendor Class Identifier (PXEClient)
    let vendor_class = b"PXEClient";
    options.push(DHCP_OPTION_VENDOR_CLASS);
    options.push(vendor_class.len() as u8);
    options.extend_from_slice(vendor_class);

    if is_ipxe {
        // For iPXE clients, send the boot script URL via option 175
        let script_url = format!("http://{}:{}/boot", config.server_ip, config.http_port);
        let script_bytes = script_url.as_bytes();

        // Option 175: iPXE encapsulated options
        // Contains sub-option 8 (script URL)
        let sub_option_len = 2 + script_bytes.len(); // 1 byte type + 1 byte len + data
        options.push(DHCP_OPTION_IPXE_ENCAP);
        options.push(sub_option_len as u8);
        options.push(IPXE_OPTION_SCRIPT);
        options.push(script_bytes.len() as u8);
        options.extend_from_slice(script_bytes);
    } else {
        // For PXE ROM clients, send TFTP server and boot file
        // Option 66: TFTP Server Name
        let server_str = config.server_ip.to_string();
        let server_bytes = server_str.as_bytes();
        options.push(DHCP_OPTION_TFTP_SERVER);
        options.push(server_bytes.len() as u8);
        options.extend_from_slice(server_bytes);

        // Option 67: Bootfile Name
        let boot_bytes = config.boot_file.as_bytes();
        options.push(DHCP_OPTION_BOOTFILE);
        options.push(boot_bytes.len() as u8);
        options.extend_from_slice(boot_bytes);
    }

    // End option
    options.push(DHCP_OPTION_END);

    // Append options to response
    response.truncate(240);
    response.extend_from_slice(&options);

    response
}

/// Build a ProxyDHCP ACK packet (similar to OFFER but with ACK type)
fn build_dhcp_ack(
    request: &[u8],
    config: &ServerConfig,
    is_ipxe: bool,
) -> Vec<u8> {
    let mut response = build_dhcp_offer(request, config, is_ipxe);
    // Find and replace the message type option
    // It's right after the magic cookie at offset 240
    if response.len() > 242 && response[240] == DHCP_OPTION_MESSAGE_TYPE {
        response[242] = DHCP_ACK;
    }
    response
}

/// Compute UDP checksum with pseudo-header
fn udp_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, udp_packet: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // Pseudo-header: src IP
    let src = src_ip.octets();
    sum += u16::from_be_bytes([src[0], src[1]]) as u32;
    sum += u16::from_be_bytes([src[2], src[3]]) as u32;

    // Pseudo-header: dst IP
    let dst = dst_ip.octets();
    sum += u16::from_be_bytes([dst[0], dst[1]]) as u32;
    sum += u16::from_be_bytes([dst[2], dst[3]]) as u32;

    // Pseudo-header: protocol (UDP = 17)
    sum += 17u32;

    // Pseudo-header: UDP length
    sum += udp_packet.len() as u32;

    // UDP header + data
    let mut i = 0;
    while i + 1 < udp_packet.len() {
        sum += u16::from_be_bytes([udp_packet[i], udp_packet[i + 1]]) as u32;
        i += 2;
    }
    // Handle odd byte
    if i < udp_packet.len() {
        sum += (udp_packet[i] as u32) << 8;
    }

    // Fold 32-bit sum to 16 bits
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }

    // One's complement
    let result = !sum as u16;
    if result == 0 { 0xffff } else { result }
}

/// Build and send a raw Ethernet frame with DHCP response
fn send_dhcp_response_raw(
    tx: &mut Box<dyn DataLinkSender>,
    src_mac: MacAddr,
    src_ip: Ipv4Addr,
    dhcp_payload: &[u8],
) -> Result<()> {
    let dst_mac = MacAddr::broadcast();
    let dst_ip = Ipv4Addr::new(255, 255, 255, 255);

    // Calculate sizes
    let udp_len = 8 + dhcp_payload.len();
    let ip_len = 20 + udp_len;
    let total_len = 14 + ip_len; // Ethernet header + IP packet

    let mut buffer = vec![0u8; total_len];

    // Build Ethernet header
    {
        let mut eth = MutableEthernetPacket::new(&mut buffer[0..14])
            .ok_or_else(|| anyhow::anyhow!("Failed to create Ethernet packet"))?;
        eth.set_destination(dst_mac);
        eth.set_source(src_mac);
        eth.set_ethertype(EtherTypes::Ipv4);
    }

    // Build IP header
    {
        let mut ip = MutableIpv4Packet::new(&mut buffer[14..14 + 20])
            .ok_or_else(|| anyhow::anyhow!("Failed to create IP packet"))?;
        ip.set_version(4);
        ip.set_header_length(5); // 20 bytes / 4
        ip.set_dscp(0);
        ip.set_ecn(0);
        ip.set_total_length(ip_len as u16);
        ip.set_identification(rand_id());
        ip.set_flags(0);
        ip.set_fragment_offset(0);
        ip.set_ttl(64);
        ip.set_next_level_protocol(IpNextHeaderProtocols::Udp);
        ip.set_source(src_ip);
        ip.set_destination(dst_ip);
        ip.set_checksum(0);
        let checksum = ipv4_checksum(&ip.to_immutable());
        ip.set_checksum(checksum);
    }

    // Build UDP header and payload
    let udp_start = 14 + 20;
    {
        // Copy payload first
        buffer[udp_start + 8..udp_start + 8 + dhcp_payload.len()].copy_from_slice(dhcp_payload);

        let mut udp = MutableUdpPacket::new(&mut buffer[udp_start..udp_start + udp_len])
            .ok_or_else(|| anyhow::anyhow!("Failed to create UDP packet"))?;
        udp.set_source(DHCP_SERVER_PORT);
        udp.set_destination(DHCP_CLIENT_PORT);
        udp.set_length(udp_len as u16);
        udp.set_checksum(0);
    }

    // UDP checksum is optional for IPv4 (RFC 768)
    // Setting to 0 disables checksum validation, avoiding offload issues
    // The checksum field is already 0 from set_checksum(0) above

    // Send the packet
    tx.send_to(&buffer, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to send packet"))?
        .map_err(|e| anyhow::anyhow!("Send error: {}", e))?;

    Ok(())
}

/// Generate a random IP identification number
fn rand_id() -> u16 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos & 0xffff) as u16
}

// ============================================================================
// HTTP Server for iPXE boot scripts
// ============================================================================

/// Parse the MAC address from query string like "mac=aa:bb:cc:dd:ee:ff"
fn parse_mac_from_query(query: &str) -> Option<String> {
    for param in query.split('&') {
        if let Some(value) = param.strip_prefix("mac=") {
            return Some(normalize_mac(value));
        }
    }
    None
}

/// Generate an iPXE script for a machine
fn generate_boot_script(mac: &str) -> String {
    let entries = match read_boot_entries() {
        Ok(e) => e,
        Err(_) => return "#!ipxe\nexit\n".to_string(),
    };

    // Find boot assignment for this MAC
    if let Some(idx) = find_boot_by_mac(&entries, mac) {
        let profile_name = &entries[idx].profile;

        // Try to read the profile
        if let Ok(script) = read_profile(profile_name) {
            return script;
        }

        // Profile not found, return error script
        return format!(
            "#!ipxe\necho Profile '{}' not found\nsleep 5\nexit\n",
            profile_name
        );
    }

    // No assignment, boot local
    "#!ipxe\nexit\n".to_string()
}

/// Handle the /done endpoint - remove boot assignment
fn handle_done(mac: &str) -> String {
    let mut entries = match read_boot_entries() {
        Ok(e) => e,
        Err(_) => return "error".to_string(),
    };

    if let Some(idx) = find_boot_by_mac(&entries, mac) {
        let removed = entries.remove(idx);
        if write_boot_entries(&entries).is_ok() {
            eprintln!("HTTP /done: removed assignment '{}' from {}", removed.profile, mac);
            return "ok".to_string();
        }
    }

    "not_found".to_string()
}

/// Handle an HTTP request
fn handle_http_request(mut stream: TcpStream) {
    let buf_reader = BufReader::new(&stream);
    let request_line = match buf_reader.lines().next() {
        Some(Ok(line)) => line,
        _ => return,
    };

    // Parse request line: "GET /path?query HTTP/1.1"
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "GET" {
        let response = "HTTP/1.1 405 Method Not Allowed\r\n\r\n";
        let _ = stream.write_all(response.as_bytes());
        return;
    }

    let path_query = parts[1];
    let (path, query) = match path_query.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path_query, None),
    };

    let (status, content_type, body) = match path {
        "/boot" => {
            if let Some(q) = query {
                if let Some(mac) = parse_mac_from_query(q) {
                    eprintln!("HTTP /boot: {}", mac);
                    let script = generate_boot_script(&mac);
                    ("200 OK", "text/plain", script)
                } else {
                    ("400 Bad Request", "text/plain", "Missing mac parameter".to_string())
                }
            } else {
                ("400 Bad Request", "text/plain", "Missing mac parameter".to_string())
            }
        }
        "/done" => {
            if let Some(q) = query {
                if let Some(mac) = parse_mac_from_query(q) {
                    let result = handle_done(&mac);
                    ("200 OK", "text/plain", result)
                } else {
                    ("400 Bad Request", "text/plain", "Missing mac parameter".to_string())
                }
            } else {
                ("400 Bad Request", "text/plain", "Missing mac parameter".to_string())
            }
        }
        "/health" => {
            ("200 OK", "text/plain", "ok".to_string())
        }
        _ => {
            ("404 Not Found", "text/plain", "Not Found".to_string())
        }
    };

    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        content_type,
        body.len(),
        body
    );

    let _ = stream.write_all(response.as_bytes());
}

/// Start the HTTP server
fn start_http_server(bind_addr: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(bind_addr)
        .context(format!("Failed to bind HTTP server to {}", bind_addr))?;

    eprintln!("HTTP server listening on {}", bind_addr);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    handle_http_request(stream);
                });
            }
            Err(e) => {
                eprintln!("HTTP connection error: {}", e);
            }
        }
    }

    Ok(())
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

/// Information about a PXE DHCP request
#[derive(Debug, PartialEq)]
struct PxeRequest {
    mac: String,
    message_type: u8,
    is_ipxe: bool,
}

fn handle_dhcp_packet(dhcp_data: &[u8]) -> Option<PxeRequest> {
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
    let (message_type, vendor_class, user_class) = parse_dhcp_options(dhcp_data);

    // Only process DHCP DISCOVER or REQUEST with PXE vendor class
    let msg_type = message_type?;
    if msg_type != DHCP_DISCOVER && msg_type != DHCP_REQUEST {
        return None;
    }

    if !is_pxe_request(&vendor_class) {
        return None;
    }

    let is_ipxe = is_ipxe_request(&user_class);

    Some(PxeRequest {
        mac,
        message_type: msg_type,
        is_ipxe,
    })
}

/// Result of processing a packet - includes the request info and raw DHCP data
struct ProcessedPacket {
    request: PxeRequest,
    dhcp_data: Vec<u8>,
}

fn process_packet(ethernet: &EthernetPacket) -> Option<ProcessedPacket> {
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
                            let dhcp_data = udp.payload().to_vec();
                            if let Some(request) = handle_dhcp_packet(&dhcp_data) {
                                return Some(ProcessedPacket { request, dhcp_data });
                            }
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

fn run_listener(args: &Args) -> Result<()> {
    let interface = if let Some(name) = &args.interface {
        datalink::interfaces()
            .into_iter()
            .find(|iface| iface.name == *name)
            .ok_or_else(|| anyhow::anyhow!("Interface '{}' not found", name))?
    } else {
        find_default_interface()
            .ok_or_else(|| anyhow::anyhow!("No suitable network interface found"))?
    };

    // Get server IP from interface
    let server_ip = get_interface_ip(&interface)
        .ok_or_else(|| anyhow::anyhow!("Interface '{}' has no IPv4 address", interface.name))?;

    let config = ServerConfig {
        server_ip,
        http_port: args.http_port,
        boot_file: args.boot_file.clone(),
        respond: !args.no_respond,
        interface_name: interface.name.clone(),
    };

    eprintln!("serabutd starting on interface: {} [fix raw-pkt-udp-cksum-zero: attempt #5]", interface.name);
    eprintln!("Server IP: {}", server_ip);
    if config.respond {
        eprintln!("ProxyDHCP responses: enabled");
        eprintln!("TFTP boot file: {}", config.boot_file);
        eprintln!("HTTP endpoint: http://{}:{}/boot", server_ip, config.http_port);
    } else {
        eprintln!("ProxyDHCP responses: disabled (listen-only mode)");
    }

    ensure_data_dir()?;

    // Start HTTP server in a separate thread
    let http_port = config.http_port;
    thread::spawn(move || {
        let bind_addr = SocketAddr::from(([0, 0, 0, 0], http_port));
        if let Err(e) = start_http_server(bind_addr) {
            eprintln!("HTTP server error: {}", e);
        }
    });

    eprintln!("Listening for PXE boot requests...");

    // Get interface MAC address
    let src_mac = interface
        .mac
        .ok_or_else(|| anyhow::anyhow!("Interface '{}' has no MAC address", interface.name))?;
    eprintln!("Interface MAC: {}", src_mac);

    // Create datalink channel for raw packet I/O
    // Using raw packets allows us to compute checksums in software,
    // avoiding issues with checksum offload on virtual bridges
    let (mut tx, mut rx) = match datalink::channel(&interface, Default::default()) {
        Ok(Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => return Err(anyhow::anyhow!("Unhandled channel type")),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to create datalink channel: {}. Try running as root or with CAP_NET_RAW.",
                e
            ))
        }
    };

    // Wrap tx in Arc<Mutex> for use in the main loop
    let tx = Arc::new(Mutex::new(tx));

    loop {
        match rx.next() {
            Ok(packet) => {
                if let Some(ethernet) = EthernetPacket::new(packet) {
                    if let Some(processed) = process_packet(&ethernet) {
                        let req = &processed.request;
                        let client_type = if req.is_ipxe { "iPXE" } else { "PXE ROM" };
                        let msg_type_str = match req.message_type {
                            DHCP_DISCOVER => "DISCOVER",
                            DHCP_REQUEST => "REQUEST",
                            _ => "UNKNOWN",
                        };

                        eprintln!(
                            "PXE {} from {} [{}]",
                            msg_type_str, req.mac, client_type
                        );

                        // Update mac.txt
                        match read_mac_entries() {
                            Ok(mut entries) => {
                                update_or_insert_mac(&mut entries, &req.mac);
                                if let Err(e) = write_mac_entries(&entries) {
                                    eprintln!("  Failed to write mac.txt: {}", e);
                                }
                            }
                            Err(e) => {
                                eprintln!("  Failed to read mac.txt: {}", e);
                            }
                        }

                        // Send ProxyDHCP response if enabled
                        if config.respond {
                            let response = match req.message_type {
                                DHCP_DISCOVER => {
                                    build_dhcp_offer(&processed.dhcp_data, &config, req.is_ipxe)
                                }
                                DHCP_REQUEST => {
                                    build_dhcp_ack(&processed.dhcp_data, &config, req.is_ipxe)
                                }
                                _ => continue,
                            };

                            let resp_type = if req.message_type == DHCP_DISCOVER {
                                "OFFER"
                            } else {
                                "ACK"
                            };

                            // Send raw packet with proper checksums
                            let mut tx_guard = tx.lock().unwrap();
                            match send_dhcp_response_raw(
                                &mut *tx_guard,
                                src_mac,
                                config.server_ip,
                                &response,
                            ) {
                                Ok(_) => {
                                    if req.is_ipxe {
                                        eprintln!(
                                            "  Sent {} with script URL: http://{}:{}/boot",
                                            resp_type, config.server_ip, config.http_port
                                        );
                                    } else {
                                        eprintln!(
                                            "  Sent {} with boot file: {}",
                                            resp_type, config.boot_file
                                        );
                                    }
                                }
                                Err(e) => {
                                    eprintln!("  Failed to send {}: {}", resp_type, e);
                                }
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

    run_listener(&args).context("Failed to run listener")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a minimal DHCP packet
    fn create_dhcp_packet(
        op: u8,
        htype: u8,
        hlen: u8,
        mac: [u8; 6],
        message_type: Option<u8>,
        vendor_class: Option<&str>,
        user_class: Option<&str>,
    ) -> Vec<u8> {
        let mut packet = vec![0u8; 240];

        // Basic DHCP header
        packet[0] = op; // op
        packet[1] = htype; // htype
        packet[2] = hlen; // hlen
        // bytes 3-27 are zeros (hops, xid, secs, flags, addresses)

        // MAC address at offset 28
        packet[28..34].copy_from_slice(&mac);

        // Magic cookie at offset 236
        packet[236] = 99;
        packet[237] = 130;
        packet[238] = 83;
        packet[239] = 99;

        // Options start at 240
        if let Some(mt) = message_type {
            packet.push(DHCP_OPTION_MESSAGE_TYPE);
            packet.push(1); // length
            packet.push(mt);
        }

        if let Some(vc) = vendor_class {
            packet.push(DHCP_OPTION_VENDOR_CLASS);
            packet.push(vc.len() as u8);
            packet.extend_from_slice(vc.as_bytes());
        }

        if let Some(uc) = user_class {
            packet.push(DHCP_OPTION_USER_CLASS);
            packet.push(uc.len() as u8);
            packet.extend_from_slice(uc.as_bytes());
        }

        packet.push(DHCP_OPTION_END);

        packet
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

    mod parse_dhcp_options_tests {
        use super::*;

        #[test]
        fn parses_message_type() {
            let packet = create_dhcp_packet(
                1,
                1,
                6,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(DHCP_DISCOVER),
                None,
                None,
            );
            let (msg_type, _, _) = parse_dhcp_options(&packet);
            assert_eq!(msg_type, Some(DHCP_DISCOVER));
        }

        #[test]
        fn parses_vendor_class() {
            let packet = create_dhcp_packet(
                1,
                1,
                6,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(DHCP_DISCOVER),
                Some("PXEClient:Arch:00007"),
                None,
            );
            let (_, vendor_class, _) = parse_dhcp_options(&packet);
            assert_eq!(vendor_class, Some("PXEClient:Arch:00007".to_string()));
        }

        #[test]
        fn parses_user_class_ipxe() {
            let packet = create_dhcp_packet(
                1,
                1,
                6,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(DHCP_DISCOVER),
                Some("PXEClient:Arch:00007"),
                Some("iPXE"),
            );
            let (_, _, user_class) = parse_dhcp_options(&packet);
            assert_eq!(user_class, Some("iPXE".to_string()));
        }

        #[test]
        fn returns_none_for_short_packet() {
            let packet = vec![0u8; 100]; // Too short
            let (msg_type, vendor_class, user_class) = parse_dhcp_options(&packet);
            assert!(msg_type.is_none());
            assert!(vendor_class.is_none());
            assert!(user_class.is_none());
        }

        #[test]
        fn returns_none_for_bad_magic_cookie() {
            let mut packet = vec![0u8; 250];
            // Wrong magic cookie
            packet[236] = 0;
            packet[237] = 0;
            packet[238] = 0;
            packet[239] = 0;
            let (msg_type, _, _) = parse_dhcp_options(&packet);
            assert!(msg_type.is_none());
        }
    }

    mod is_pxe_request_tests {
        use super::*;

        #[test]
        fn detects_pxe_client() {
            let vc = Some("PXEClient:Arch:00007:UNDI:003016".to_string());
            assert!(is_pxe_request(&vc));
        }

        #[test]
        fn rejects_non_pxe() {
            let vc = Some("MSFT 5.0".to_string());
            assert!(!is_pxe_request(&vc));
        }

        #[test]
        fn rejects_none() {
            assert!(!is_pxe_request(&None));
        }
    }

    mod is_ipxe_request_tests {
        use super::*;

        #[test]
        fn detects_ipxe() {
            let uc = Some("iPXE".to_string());
            assert!(is_ipxe_request(&uc));
        }

        #[test]
        fn detects_ipxe_in_longer_string() {
            let uc = Some("iPXE/1.0.0".to_string());
            assert!(is_ipxe_request(&uc));
        }

        #[test]
        fn rejects_non_ipxe() {
            let uc = Some("PXEClient".to_string());
            assert!(!is_ipxe_request(&uc));
        }

        #[test]
        fn rejects_none() {
            assert!(!is_ipxe_request(&None));
        }
    }

    mod handle_dhcp_packet_tests {
        use super::*;

        #[test]
        fn accepts_pxe_discover() {
            let packet = create_dhcp_packet(
                1, // BOOTREQUEST
                1, // Ethernet
                6, // MAC length
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(DHCP_DISCOVER),
                Some("PXEClient:Arch:00007"),
                None,
            );
            let result = handle_dhcp_packet(&packet).unwrap();
            assert_eq!(result.mac, "aa:bb:cc:dd:ee:ff");
            assert_eq!(result.message_type, DHCP_DISCOVER);
            assert!(!result.is_ipxe);
        }

        #[test]
        fn accepts_pxe_request() {
            let packet = create_dhcp_packet(
                1, // BOOTREQUEST
                1, // Ethernet
                6, // MAC length
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(DHCP_REQUEST),
                Some("PXEClient:Arch:00007"),
                None,
            );
            let result = handle_dhcp_packet(&packet).unwrap();
            assert_eq!(result.mac, "aa:bb:cc:dd:ee:ff");
            assert_eq!(result.message_type, DHCP_REQUEST);
            assert!(!result.is_ipxe);
        }

        #[test]
        fn detects_ipxe_client() {
            let packet = create_dhcp_packet(
                1,
                1,
                6,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(DHCP_DISCOVER),
                Some("PXEClient:Arch:00007"),
                Some("iPXE"),
            );
            let result = handle_dhcp_packet(&packet).unwrap();
            assert!(result.is_ipxe);
        }

        #[test]
        fn rejects_non_pxe_discover() {
            let packet = create_dhcp_packet(
                1,
                1,
                6,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(DHCP_DISCOVER),
                Some("MSFT 5.0"), // Not PXE
                None,
            );
            let result = handle_dhcp_packet(&packet);
            assert!(result.is_none());
        }

        #[test]
        fn rejects_dhcp_offer() {
            let packet = create_dhcp_packet(
                1,
                1,
                6,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(DHCP_OFFER), // Not DISCOVER or REQUEST
                Some("PXEClient:Arch:00007"),
                None,
            );
            let result = handle_dhcp_packet(&packet);
            assert!(result.is_none());
        }

        #[test]
        fn rejects_reply_packets() {
            let packet = create_dhcp_packet(
                2, // BOOTREPLY, not BOOTREQUEST
                1,
                6,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(DHCP_DISCOVER),
                Some("PXEClient:Arch:00007"),
                None,
            );
            let result = handle_dhcp_packet(&packet);
            assert!(result.is_none());
        }

        #[test]
        fn rejects_non_ethernet() {
            let packet = create_dhcp_packet(
                1,
                6, // Not ethernet (6 = IEEE 802)
                6,
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                Some(DHCP_DISCOVER),
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
            assert_eq!(DHCP_DISCOVER, 1);
        }

        #[test]
        fn offer_is_two() {
            // RFC 2132 defines DHCPOFFER as 2
            assert_eq!(DHCP_OFFER, 2);
        }

        #[test]
        fn request_is_three() {
            // RFC 2132 defines DHCPREQUEST as 3
            assert_eq!(DHCP_REQUEST, 3);
        }
    }
}
