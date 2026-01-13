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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_packet(op: u8, options: Vec<DhcpOption>) -> DhcpPacket {
        DhcpPacket {
            op,
            htype: 1,
            hlen: 6,
            xid: 0x12345678,
            secs: 0,
            flags: 0,
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr: MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff),
            sname: None,
            file: None,
            options,
        }
    }

    mod dhcp_message_type_tests {
        use super::*;

        #[test]
        fn test_from_u8_valid_values() {
            assert_eq!(DhcpMessageType::from_u8(1), Some(DhcpMessageType::Discover));
            assert_eq!(DhcpMessageType::from_u8(2), Some(DhcpMessageType::Offer));
            assert_eq!(DhcpMessageType::from_u8(3), Some(DhcpMessageType::Request));
            assert_eq!(DhcpMessageType::from_u8(4), Some(DhcpMessageType::Decline));
            assert_eq!(DhcpMessageType::from_u8(5), Some(DhcpMessageType::Ack));
            assert_eq!(DhcpMessageType::from_u8(6), Some(DhcpMessageType::Nak));
            assert_eq!(DhcpMessageType::from_u8(7), Some(DhcpMessageType::Release));
            assert_eq!(DhcpMessageType::from_u8(8), Some(DhcpMessageType::Inform));
        }

        #[test]
        fn test_from_u8_invalid_values() {
            assert_eq!(DhcpMessageType::from_u8(0), None);
            assert_eq!(DhcpMessageType::from_u8(9), None);
            assert_eq!(DhcpMessageType::from_u8(255), None);
        }

        #[test]
        fn test_display() {
            assert_eq!(format!("{}", DhcpMessageType::Discover), "DISCOVER");
            assert_eq!(format!("{}", DhcpMessageType::Offer), "OFFER");
            assert_eq!(format!("{}", DhcpMessageType::Request), "REQUEST");
            assert_eq!(format!("{}", DhcpMessageType::Decline), "DECLINE");
            assert_eq!(format!("{}", DhcpMessageType::Ack), "ACK");
            assert_eq!(format!("{}", DhcpMessageType::Nak), "NAK");
            assert_eq!(format!("{}", DhcpMessageType::Release), "RELEASE");
            assert_eq!(format!("{}", DhcpMessageType::Inform), "INFORM");
        }

        #[test]
        fn test_clone_and_copy() {
            let msg = DhcpMessageType::Discover;
            let cloned = msg.clone();
            let copied = msg;
            assert_eq!(msg, cloned);
            assert_eq!(msg, copied);
        }

        #[test]
        fn test_equality() {
            assert_eq!(DhcpMessageType::Discover, DhcpMessageType::Discover);
            assert_ne!(DhcpMessageType::Discover, DhcpMessageType::Offer);
        }
    }

    mod dhcp_packet_tests {
        use super::*;

        #[test]
        fn test_is_request() {
            let packet = create_test_packet(1, vec![]);
            assert!(packet.is_request());
            assert!(!packet.is_reply());
        }

        #[test]
        fn test_is_reply() {
            let packet = create_test_packet(2, vec![]);
            assert!(packet.is_reply());
            assert!(!packet.is_request());
        }

        #[test]
        fn test_message_type_present() {
            let packet = create_test_packet(
                1,
                vec![DhcpOption::MessageType(DhcpMessageType::Discover)],
            );
            assert_eq!(packet.message_type(), Some(DhcpMessageType::Discover));
        }

        #[test]
        fn test_message_type_absent() {
            let packet = create_test_packet(1, vec![]);
            assert_eq!(packet.message_type(), None);
        }

        #[test]
        fn test_message_type_among_other_options() {
            let packet = create_test_packet(
                1,
                vec![
                    DhcpOption::VendorClassId("PXEClient".to_string()),
                    DhcpOption::MessageType(DhcpMessageType::Request),
                    DhcpOption::ClientArch(7),
                ],
            );
            assert_eq!(packet.message_type(), Some(DhcpMessageType::Request));
        }

        #[test]
        fn test_vendor_class_id_present() {
            let packet = create_test_packet(
                1,
                vec![DhcpOption::VendorClassId("PXEClient:Arch:00007".to_string())],
            );
            assert_eq!(packet.vendor_class_id(), Some("PXEClient:Arch:00007"));
        }

        #[test]
        fn test_vendor_class_id_absent() {
            let packet = create_test_packet(1, vec![]);
            assert_eq!(packet.vendor_class_id(), None);
        }

        #[test]
        fn test_client_arch_present() {
            let packet = create_test_packet(1, vec![DhcpOption::ClientArch(7)]);
            assert_eq!(packet.client_arch(), Some(7));
        }

        #[test]
        fn test_client_arch_absent() {
            let packet = create_test_packet(1, vec![]);
            assert_eq!(packet.client_arch(), None);
        }

        #[test]
        fn test_client_uuid_present() {
            let uuid = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
            let packet = create_test_packet(1, vec![DhcpOption::ClientUuid(uuid.clone())]);
            assert_eq!(packet.client_uuid(), Some(uuid.as_slice()));
        }

        #[test]
        fn test_client_uuid_absent() {
            let packet = create_test_packet(1, vec![]);
            assert_eq!(packet.client_uuid(), None);
        }

        #[test]
        fn test_all_accessors_with_full_options() {
            let packet = create_test_packet(
                1,
                vec![
                    DhcpOption::MessageType(DhcpMessageType::Discover),
                    DhcpOption::VendorClassId("PXEClient".to_string()),
                    DhcpOption::ClientArch(7),
                    DhcpOption::ClientUuid(vec![0x01, 0x02]),
                    DhcpOption::RequestedIp(Ipv4Addr::new(192, 168, 1, 100)),
                    DhcpOption::ServerIdentifier(Ipv4Addr::new(192, 168, 1, 1)),
                    DhcpOption::ClientId(vec![0x01, 0xaa, 0xbb]),
                    DhcpOption::ClientNdi(vec![0x01, 0x03, 0x10]),
                    DhcpOption::Unknown(200, vec![0x01, 0x02]),
                ],
            );

            assert_eq!(packet.message_type(), Some(DhcpMessageType::Discover));
            assert_eq!(packet.vendor_class_id(), Some("PXEClient"));
            assert_eq!(packet.client_arch(), Some(7));
            assert_eq!(packet.client_uuid(), Some(&[0x01, 0x02][..]));
        }

        #[test]
        fn test_clone() {
            let packet = create_test_packet(
                1,
                vec![DhcpOption::MessageType(DhcpMessageType::Discover)],
            );
            let cloned = packet.clone();

            assert_eq!(cloned.op, packet.op);
            assert_eq!(cloned.xid, packet.xid);
            assert_eq!(cloned.chaddr, packet.chaddr);
            assert_eq!(cloned.message_type(), packet.message_type());
        }
    }

    mod dhcp_option_tests {
        use super::*;

        #[test]
        fn test_option_variants() {
            // Just ensure all variants can be constructed
            let _ = DhcpOption::MessageType(DhcpMessageType::Discover);
            let _ = DhcpOption::RequestedIp(Ipv4Addr::new(192, 168, 1, 1));
            let _ = DhcpOption::ServerIdentifier(Ipv4Addr::new(192, 168, 1, 1));
            let _ = DhcpOption::VendorClassId("test".to_string());
            let _ = DhcpOption::ClientId(vec![0x01]);
            let _ = DhcpOption::ClientArch(7);
            let _ = DhcpOption::ClientNdi(vec![0x01]);
            let _ = DhcpOption::ClientUuid(vec![0x01]);
            let _ = DhcpOption::Unknown(100, vec![0x01]);
        }

        #[test]
        fn test_option_clone() {
            let opt = DhcpOption::VendorClassId("PXEClient".to_string());
            let cloned = opt.clone();
            if let DhcpOption::VendorClassId(s) = cloned {
                assert_eq!(s, "PXEClient");
            } else {
                panic!("Clone changed variant");
            }
        }
    }
}
