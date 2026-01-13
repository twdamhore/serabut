//! DHCP packet domain models.
//!
//! These types represent the logical structure of DHCP packets,
//! independent of wire format parsing (SRP).

use std::net::Ipv4Addr;

use macaddr::MacAddr6;

/// DHCP message types as defined in RFC 2131.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DhcpMessageType {
    Discover,
    Offer,
    Request,
    Decline,
    Ack,
    Nak,
    Release,
    Inform,
}

impl DhcpMessageType {
    /// Parse from the DHCP option 53 value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Discover),
            2 => Some(Self::Offer),
            3 => Some(Self::Request),
            4 => Some(Self::Decline),
            5 => Some(Self::Ack),
            6 => Some(Self::Nak),
            7 => Some(Self::Release),
            8 => Some(Self::Inform),
            _ => None,
        }
    }
}

impl std::fmt::Display for DhcpMessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discover => write!(f, "DISCOVER"),
            Self::Offer => write!(f, "OFFER"),
            Self::Request => write!(f, "REQUEST"),
            Self::Decline => write!(f, "DECLINE"),
            Self::Ack => write!(f, "ACK"),
            Self::Nak => write!(f, "NAK"),
            Self::Release => write!(f, "RELEASE"),
            Self::Inform => write!(f, "INFORM"),
        }
    }
}

/// Relevant DHCP options we care about for PXE detection.
#[derive(Debug, Clone)]
pub enum DhcpOption {
    /// Option 53: DHCP Message Type
    MessageType(DhcpMessageType),
    /// Option 50: Requested IP Address
    RequestedIp(Ipv4Addr),
    /// Option 54: Server Identifier
    ServerIdentifier(Ipv4Addr),
    /// Option 60: Vendor Class Identifier (e.g., "PXEClient:...")
    VendorClassId(String),
    /// Option 61: Client Identifier
    ClientId(Vec<u8>),
    /// Option 93: Client System Architecture (PXE)
    ClientArch(u16),
    /// Option 94: Client Network Interface Identifier (PXE)
    ClientNdi(Vec<u8>),
    /// Option 97: Client Machine Identifier (UUID/GUID)
    ClientUuid(Vec<u8>),
    /// Unknown option (code, data)
    Unknown(u8, Vec<u8>),
}

/// A parsed DHCP packet with all relevant fields.
#[derive(Debug, Clone)]
pub struct DhcpPacket {
    /// Operation: 1 = BOOTREQUEST, 2 = BOOTREPLY
    pub op: u8,
    /// Hardware type (1 = Ethernet)
    pub htype: u8,
    /// Hardware address length
    pub hlen: u8,
    /// Transaction ID
    pub xid: u32,
    /// Seconds elapsed
    pub secs: u16,
    /// Flags
    pub flags: u16,
    /// Client IP address (if already known)
    pub ciaddr: Ipv4Addr,
    /// 'Your' IP address (assigned by server)
    pub yiaddr: Ipv4Addr,
    /// Server IP address
    pub siaddr: Ipv4Addr,
    /// Gateway IP address
    pub giaddr: Ipv4Addr,
    /// Client hardware address (MAC)
    pub chaddr: MacAddr6,
    /// Server hostname (optional)
    pub sname: Option<String>,
    /// Boot filename (optional)
    pub file: Option<String>,
    /// DHCP options
    pub options: Vec<DhcpOption>,
}

impl DhcpPacket {
    /// Returns true if this is a client request (BOOTREQUEST).
    pub fn is_request(&self) -> bool {
        self.op == 1
    }

    /// Returns true if this is a server reply (BOOTREPLY).
    pub fn is_reply(&self) -> bool {
        self.op == 2
    }

    /// Get the DHCP message type from options.
    pub fn message_type(&self) -> Option<DhcpMessageType> {
        self.options.iter().find_map(|opt| {
            if let DhcpOption::MessageType(msg_type) = opt {
                Some(*msg_type)
            } else {
                None
            }
        })
    }

    /// Get the vendor class identifier (Option 60).
    pub fn vendor_class_id(&self) -> Option<&str> {
        self.options.iter().find_map(|opt| {
            if let DhcpOption::VendorClassId(ref s) = opt {
                Some(s.as_str())
            } else {
                None
            }
        })
    }

    /// Get the client architecture type (Option 93).
    pub fn client_arch(&self) -> Option<u16> {
        self.options.iter().find_map(|opt| {
            if let DhcpOption::ClientArch(arch) = opt {
                Some(*arch)
            } else {
                None
            }
        })
    }

    /// Get the client UUID (Option 97).
    pub fn client_uuid(&self) -> Option<&[u8]> {
        self.options.iter().find_map(|opt| {
            if let DhcpOption::ClientUuid(ref uuid) = opt {
                Some(uuid.as_slice())
            } else {
                None
            }
        })
    }
}
