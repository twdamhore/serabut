use std::env;
use std::ffi::CStr;
use std::net::{Ipv4Addr, UdpSocket};
use std::os::raw::{c_char, c_uint, c_void};
use std::ptr;

const DHCP_SERVER_PORT: u16 = 4011;
const DHCP_MAGIC: [u8; 4] = [99, 130, 83, 99];
const DHCP_OPTION_END: u8 = 255;
const DHCP_OPTION_PAD: u8 = 0;
const DHCP_OPTION_MESSAGE_TYPE: u8 = 53;
const DHCP_OPTION_VENDOR_CLASS: u8 = 60;
const DHCP_OPTION_CLIENT_ARCH: u8 = 93;
const DHCP_OPTION_SERVER_ID: u8 = 54;
const DHCP_OPTION_TFTP_SERVER: u8 = 66;
const DHCP_OPTION_BOOTFILE: u8 = 67;

const DHCPDISCOVER: u8 = 1;
const DHCPREQUEST: u8 = 3;
const DHCPOFFER: u8 = 2;

const AF_INET: u16 = 2;

#[repr(C)]
struct ifaddrs {
    ifa_next: *mut ifaddrs,
    ifa_name: *mut c_char,
    ifa_flags: c_uint,
    ifa_addr: *mut sockaddr,
    ifa_netmask: *mut sockaddr,
    ifa_ifu: *mut sockaddr,
    ifa_data: *mut c_void,
}

#[repr(C)]
struct sockaddr {
    sa_family: u16,
    sa_data: [u8; 14],
}

#[repr(C)]
struct in_addr {
    s_addr: u32,
}

#[repr(C)]
struct sockaddr_in {
    sin_family: u16,
    sin_port: u16,
    sin_addr: in_addr,
    sin_zero: [u8; 8],
}

extern "C" {
    fn getifaddrs(addrs: *mut *mut ifaddrs) -> i32;
    fn freeifaddrs(addrs: *mut ifaddrs);
}

struct DhcpRequest {
    xid: u32,
    htype: u8,
    hlen: u8,
    flags: u16,
    chaddr: [u8; 16],
    message_type: u8,
    vendor_class: Option<Vec<u8>>,
    client_arch: Option<u16>,
}

fn usage() -> String {
    let name = env::args().next().unwrap_or_else(|| "pxe-proxy".to_string());
    format!(
        "Usage: {name} -i <iface>\n\n"
    )
}

fn parse_args() -> Result<String, String> {
    let mut args = env::args().skip(1);
    let mut iface: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-i" | "--iface" => {
                iface = args.next();
                if iface.is_none() {
                    return Err("Missing interface name after -i".to_string());
                }
            }
            "-h" | "--help" => return Err(usage()),
            _ => return Err(format!("Unknown argument: {arg}\n\n{}", usage())),
        }
    }

    iface.ok_or_else(|| format!("Interface is required.\n\n{}", usage()))
}

fn get_ipv4_for_iface(iface: &str) -> Option<Ipv4Addr> {
    unsafe {
        let mut ifap: *mut ifaddrs = ptr::null_mut();
        if getifaddrs(&mut ifap) != 0 {
            return None;
        }

        let mut cur = ifap;
        let mut found = None;

        while !cur.is_null() {
            let ifa = &*cur;
            if !ifa.ifa_name.is_null() && !ifa.ifa_addr.is_null() {
                let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy();
                if name == iface && (*ifa.ifa_addr).sa_family == AF_INET {
                    let addr = &*(ifa.ifa_addr as *const sockaddr_in);
                    let raw = u32::from_be(addr.sin_addr.s_addr);
                    found = Some(Ipv4Addr::from(raw));
                    break;
                }
            }
            cur = ifa.ifa_next;
        }

        freeifaddrs(ifap);
        found
    }
}

fn parse_dhcp_request(buf: &[u8]) -> Option<DhcpRequest> {
    if buf.len() < 240 {
        return None;
    }

    if buf[236..240] != DHCP_MAGIC {
        return None;
    }

    let htype = buf[1];
    let hlen = buf[2];
    let xid = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let flags = u16::from_be_bytes([buf[10], buf[11]]);
    let mut chaddr = [0u8; 16];
    chaddr.copy_from_slice(&buf[28..44]);

    let mut message_type = 0u8;
    let mut vendor_class = None;
    let mut client_arch = None;

    let mut idx = 240usize;
    while idx < buf.len() {
        let code = buf[idx];
        idx += 1;

        if code == DHCP_OPTION_PAD {
            continue;
        }

        if code == DHCP_OPTION_END {
            break;
        }

        if idx >= buf.len() {
            break;
        }
        let len = buf[idx] as usize;
        idx += 1;
        if idx + len > buf.len() {
            break;
        }
        let data = &buf[idx..idx + len];
        match code {
            DHCP_OPTION_MESSAGE_TYPE => {
                if !data.is_empty() {
                    message_type = data[0];
                }
            }
            DHCP_OPTION_VENDOR_CLASS => vendor_class = Some(data.to_vec()),
            DHCP_OPTION_CLIENT_ARCH => {
                if data.len() >= 2 {
                    client_arch = Some(u16::from_be_bytes([data[0], data[1]]));
                }
            }
            _ => {}
        }
        idx += len;
    }

    Some(DhcpRequest {
        xid,
        htype,
        hlen,
        flags,
        chaddr,
        message_type,
        vendor_class,
        client_arch,
    })
}

fn is_pxe_request(req: &DhcpRequest) -> bool {
    match &req.vendor_class {
        Some(vendor) => vendor.starts_with(b"PXEClient"),
        None => false,
    }
}

fn pick_bootfile(arch: Option<u16>) -> &'static str {
    match arch {
        Some(0) => "undionly.kpxe",
        Some(7) | Some(9) => "ipxe.efi",
        Some(11) => "ipxe.efi",
        _ => "ipxe.efi",
    }
}

fn format_mac(chaddr: &[u8; 16], hlen: u8) -> String {
    let len = hlen.min(16) as usize;
    let mut parts = Vec::with_capacity(len);
    for i in 0..len {
        parts.push(format!("{:02x}", chaddr[i]));
    }
    parts.join(":")
}

fn push_option(buf: &mut Vec<u8>, code: u8, data: &[u8]) {
    buf.push(code);
    buf.push(data.len() as u8);
    buf.extend_from_slice(data);
}

fn build_response(req: &DhcpRequest, server_ip: Ipv4Addr, bootfile: &str) -> Vec<u8> {
    let mut buf = vec![0u8; 240];

    buf[0] = 2; // BOOTREPLY
    buf[1] = req.htype;
    buf[2] = req.hlen;
    buf[3] = 0;
    buf[4..8].copy_from_slice(&req.xid.to_be_bytes());
    buf[10..12].copy_from_slice(&req.flags.to_be_bytes());

    let ip_octets = server_ip.octets();
    buf[20..24].copy_from_slice(&ip_octets); // siaddr

    buf[28..44].copy_from_slice(&req.chaddr);

    buf[236..240].copy_from_slice(&DHCP_MAGIC);

    let mut options = Vec::new();
    push_option(&mut options, DHCP_OPTION_MESSAGE_TYPE, &[DHCPOFFER]);
    push_option(&mut options, DHCP_OPTION_SERVER_ID, &ip_octets);
    push_option(&mut options, DHCP_OPTION_TFTP_SERVER, server_ip.to_string().as_bytes());
    push_option(&mut options, DHCP_OPTION_BOOTFILE, bootfile.as_bytes());
    options.push(DHCP_OPTION_END);

    buf.extend_from_slice(&options);
    buf
}

fn main() {
    let iface = match parse_args() {
        Ok(value) => value,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    let server_ip = match get_ipv4_for_iface(&iface) {
        Some(ip) => ip,
        None => {
            eprintln!("Failed to find IPv4 for interface {iface}");
            return;
        }
    };

    let bind_addr = format!("0.0.0.0:{DHCP_SERVER_PORT}");
    let socket = match UdpSocket::bind(&bind_addr) {
        Ok(sock) => sock,
        Err(err) => {
            eprintln!("Failed to bind {bind_addr}: {err}");
            return;
        }
    };

    if let Err(err) = socket.set_broadcast(true) {
        eprintln!("Failed to set broadcast: {err}");
    }

    eprintln!("Listening on {bind_addr} (iface {iface}, ip {server_ip})");

    let mut buf = [0u8; 1500];
    loop {
        let (len, addr) = match socket.recv_from(&mut buf) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("recv_from failed: {err}");
                continue;
            }
        };

        let packet = &buf[..len];
        let req = match parse_dhcp_request(packet) {
            Some(value) => value,
            None => continue,
        };

        if req.message_type != DHCPDISCOVER && req.message_type != DHCPREQUEST {
            continue;
        }

        if !is_pxe_request(&req) {
            continue;
        }

        let bootfile = pick_bootfile(req.client_arch);
        let mac = format_mac(&req.chaddr, req.hlen);
        eprintln!(
            "PXE request from {mac} ({addr}), arch {:?} -> {bootfile}",
            req.client_arch
        );

        let response = build_response(&req, server_ip, bootfile);
        if let Err(err) = socket.send_to(&response, addr) {
            eprintln!("Failed to send response to {addr}: {err}");
        }
    }
}
