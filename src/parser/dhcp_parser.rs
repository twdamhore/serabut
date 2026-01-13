//! DHCP packet parser implementation.
//!
//! Parses raw DHCP packets according to RFC 2131.

use std::net::Ipv4Addr;

use macaddr::MacAddr6;

use crate::domain::{DhcpMessageType, DhcpOption, DhcpPacket};
use crate::error::ParseError;

/// DHCP magic cookie: 0x63825363
const DHCP_MAGIC_COOKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];

/// Minimum DHCP packet size (without options)
const MIN_DHCP_SIZE: usize = 236;

/// DHCP option codes
mod option_codes {
    pub const PAD: u8 = 0;
    pub const END: u8 = 255;
    pub const MESSAGE_TYPE: u8 = 53;
    pub const REQUESTED_IP: u8 = 50;
    pub const SERVER_ID: u8 = 54;
    pub const VENDOR_CLASS_ID: u8 = 60;
    pub const CLIENT_ID: u8 = 61;
    pub const CLIENT_ARCH: u8 = 93;
    pub const CLIENT_NDI: u8 = 94;
    pub const CLIENT_UUID: u8 = 97;
}

/// Parser for DHCP packets.
///
/// Implements the Single Responsibility Principle by focusing
/// solely on parsing DHCP wire format into domain types.
pub struct DhcpParser;

impl DhcpParser {
    /// Create a new DHCP parser.
    pub fn new() -> Self {
        Self
    }

    /// Parse a DHCP packet from raw bytes.
    ///
    /// The input should be the UDP payload (not including IP/UDP headers).
    pub fn parse(&self, data: &[u8]) -> Result<DhcpPacket, ParseError> {
        if data.len() < MIN_DHCP_SIZE {
            return Err(ParseError::PacketTooShort {
                expected: MIN_DHCP_SIZE,
                actual: data.len(),
            });
        }

        // Parse fixed header fields
        let op = data[0];
        let htype = data[1];
        let hlen = data[2];
        // hops at [3]
        let xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let secs = u16::from_be_bytes([data[8], data[9]]);
        let flags = u16::from_be_bytes([data[10], data[11]]);

        let ciaddr = Ipv4Addr::new(data[12], data[13], data[14], data[15]);
        let yiaddr = Ipv4Addr::new(data[16], data[17], data[18], data[19]);
        let siaddr = Ipv4Addr::new(data[20], data[21], data[22], data[23]);
        let giaddr = Ipv4Addr::new(data[24], data[25], data[26], data[27]);

        // Client hardware address (chaddr) - 16 bytes starting at offset 28
        // We always extract first 6 bytes as MAC (works for Ethernet htype=1, hlen=6)
        let chaddr = MacAddr6::new(
            data[28], data[29], data[30], data[31], data[32], data[33],
        );

        // Server name (sname) - 64 bytes starting at offset 44
        let sname = Self::parse_null_terminated_string(&data[44..108]);

        // Boot filename (file) - 128 bytes starting at offset 108
        let file = Self::parse_null_terminated_string(&data[108..236]);

        // Check for DHCP magic cookie at offset 236
        if data.len() < 240 {
            return Err(ParseError::PacketTooShort {
                expected: 240,
                actual: data.len(),
            });
        }

        if data[236..240] != DHCP_MAGIC_COOKIE {
            return Err(ParseError::InvalidMagicCookie);
        }

        // Parse options starting at offset 240
        let options = self.parse_options(&data[240..])?;

        Ok(DhcpPacket {
            op,
            htype,
            hlen,
            xid,
            secs,
            flags,
            ciaddr,
            yiaddr,
            siaddr,
            giaddr,
            chaddr,
            sname,
            file,
            options,
        })
    }

    /// Parse a null-terminated string, returning None if empty.
    fn parse_null_terminated_string(data: &[u8]) -> Option<String> {
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        if end == 0 {
            return None;
        }

        String::from_utf8(data[..end].to_vec()).ok()
    }

    /// Parse DHCP options from the options section.
    fn parse_options(&self, data: &[u8]) -> Result<Vec<DhcpOption>, ParseError> {
        let mut options = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let code = data[offset];

            // Handle special codes
            if code == option_codes::PAD {
                offset += 1;
                continue;
            }

            if code == option_codes::END {
                break;
            }

            // Regular option: code + length + data
            if offset + 1 >= data.len() {
                return Err(ParseError::InvalidOption {
                    offset,
                    message: "option length missing".to_string(),
                });
            }

            let len = data[offset + 1] as usize;

            if offset + 2 + len > data.len() {
                return Err(ParseError::InvalidOption {
                    offset,
                    message: format!(
                        "option data truncated: expected {} bytes, have {}",
                        len,
                        data.len() - offset - 2
                    ),
                });
            }

            let option_data = &data[offset + 2..offset + 2 + len];

            if let Some(option) = self.parse_option(code, option_data) {
                options.push(option);
            }

            offset += 2 + len;
        }

        Ok(options)
    }

    /// Parse a single DHCP option.
    fn parse_option(&self, code: u8, data: &[u8]) -> Option<DhcpOption> {
        match code {
            option_codes::MESSAGE_TYPE => {
                if data.is_empty() {
                    return None;
                }
                DhcpMessageType::from_u8(data[0]).map(DhcpOption::MessageType)
            }

            option_codes::REQUESTED_IP => {
                if data.len() < 4 {
                    return None;
                }
                Some(DhcpOption::RequestedIp(Ipv4Addr::new(
                    data[0], data[1], data[2], data[3],
                )))
            }

            option_codes::SERVER_ID => {
                if data.len() < 4 {
                    return None;
                }
                Some(DhcpOption::ServerIdentifier(Ipv4Addr::new(
                    data[0], data[1], data[2], data[3],
                )))
            }

            option_codes::VENDOR_CLASS_ID => {
                String::from_utf8(data.to_vec())
                    .ok()
                    .map(DhcpOption::VendorClassId)
            }

            option_codes::CLIENT_ID => Some(DhcpOption::ClientId(data.to_vec())),

            option_codes::CLIENT_ARCH => {
                if data.len() < 2 {
                    return None;
                }
                Some(DhcpOption::ClientArch(u16::from_be_bytes([
                    data[0], data[1],
                ])))
            }

            option_codes::CLIENT_NDI => Some(DhcpOption::ClientNdi(data.to_vec())),

            option_codes::CLIENT_UUID => Some(DhcpOption::ClientUuid(data.to_vec())),

            _ => Some(DhcpOption::Unknown(code, data.to_vec())),
        }
    }
}

impl Default for DhcpParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a valid DHCP packet with customizable fields
    fn create_test_packet() -> Vec<u8> {
        let mut packet = vec![0u8; 300];
        packet[0] = 1; // BOOTREQUEST
        packet[1] = 1; // Ethernet
        packet[2] = 6; // MAC length
        packet[4..8].copy_from_slice(&0x12345678u32.to_be_bytes());
        packet[28..34].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        packet[236..240].copy_from_slice(&DHCP_MAGIC_COOKIE);
        packet[240] = option_codes::MESSAGE_TYPE;
        packet[241] = 1;
        packet[242] = 1; // DISCOVER
        packet[243] = option_codes::END;
        packet
    }

    #[test]
    fn test_parse_minimum_packet() {
        let parser = DhcpParser::new();
        let packet = create_test_packet();

        let result = parser.parse(&packet);
        assert!(result.is_ok());

        let dhcp = result.unwrap();
        assert_eq!(dhcp.op, 1);
        assert_eq!(dhcp.xid, 0x12345678);
        assert_eq!(
            dhcp.chaddr,
            MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff)
        );
        assert_eq!(dhcp.message_type(), Some(DhcpMessageType::Discover));
    }

    #[test]
    fn test_packet_too_short() {
        let parser = DhcpParser::new();
        let packet = vec![0u8; 100];

        let result = parser.parse(&packet);
        assert!(matches!(result, Err(ParseError::PacketTooShort { .. })));
    }

    #[test]
    fn test_packet_missing_magic_cookie_space() {
        let parser = DhcpParser::new();
        let packet = vec![0u8; 238]; // Too short for magic cookie

        let result = parser.parse(&packet);
        assert!(matches!(result, Err(ParseError::PacketTooShort { .. })));
    }

    #[test]
    fn test_invalid_magic_cookie() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();
        packet[236..240].copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        let result = parser.parse(&packet);
        assert!(matches!(result, Err(ParseError::InvalidMagicCookie)));
    }

    #[test]
    fn test_parse_bootreply() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();
        packet[0] = 2; // BOOTREPLY
        packet[242] = 2; // OFFER

        let dhcp = parser.parse(&packet).unwrap();
        assert!(dhcp.is_reply());
        assert!(!dhcp.is_request());
        assert_eq!(dhcp.message_type(), Some(DhcpMessageType::Offer));
    }

    #[test]
    fn test_parse_all_message_types() {
        let parser = DhcpParser::new();

        let types = [
            (1, DhcpMessageType::Discover),
            (2, DhcpMessageType::Offer),
            (3, DhcpMessageType::Request),
            (4, DhcpMessageType::Decline),
            (5, DhcpMessageType::Ack),
            (6, DhcpMessageType::Nak),
            (7, DhcpMessageType::Release),
            (8, DhcpMessageType::Inform),
        ];

        for (code, expected) in types {
            let mut packet = create_test_packet();
            packet[242] = code;
            let dhcp = parser.parse(&packet).unwrap();
            assert_eq!(dhcp.message_type(), Some(expected));
        }
    }

    #[test]
    fn test_parse_ip_addresses() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        // ciaddr
        packet[12..16].copy_from_slice(&[192, 168, 1, 100]);
        // yiaddr
        packet[16..20].copy_from_slice(&[192, 168, 1, 101]);
        // siaddr
        packet[20..24].copy_from_slice(&[192, 168, 1, 1]);
        // giaddr
        packet[24..28].copy_from_slice(&[192, 168, 1, 254]);

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(dhcp.ciaddr, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(dhcp.yiaddr, Ipv4Addr::new(192, 168, 1, 101));
        assert_eq!(dhcp.siaddr, Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(dhcp.giaddr, Ipv4Addr::new(192, 168, 1, 254));
    }

    #[test]
    fn test_parse_secs_and_flags() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        packet[8..10].copy_from_slice(&0x1234u16.to_be_bytes());
        packet[10..12].copy_from_slice(&0x8000u16.to_be_bytes()); // Broadcast flag

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(dhcp.secs, 0x1234);
        assert_eq!(dhcp.flags, 0x8000);
    }

    #[test]
    fn test_parse_sname_field() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        let sname = b"pxeserver.local";
        packet[44..44 + sname.len()].copy_from_slice(sname);

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(dhcp.sname, Some("pxeserver.local".to_string()));
    }

    #[test]
    fn test_parse_file_field() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        let file = b"pxelinux.0";
        packet[108..108 + file.len()].copy_from_slice(file);

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(dhcp.file, Some("pxelinux.0".to_string()));
    }

    #[test]
    fn test_parse_empty_sname_and_file() {
        let parser = DhcpParser::new();
        let packet = create_test_packet();

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(dhcp.sname, None);
        assert_eq!(dhcp.file, None);
    }

    #[test]
    fn test_parse_vendor_class_id() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        let vendor_class = b"PXEClient:Arch:00007:UNDI:003016";
        packet[243] = option_codes::VENDOR_CLASS_ID;
        packet[244] = vendor_class.len() as u8;
        packet[245..245 + vendor_class.len()].copy_from_slice(vendor_class);
        packet[245 + vendor_class.len()] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(
            dhcp.vendor_class_id(),
            Some("PXEClient:Arch:00007:UNDI:003016")
        );
    }

    #[test]
    fn test_parse_client_arch() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        // Add client arch option (Option 93) - EFI x64 = 7
        packet[243] = option_codes::CLIENT_ARCH;
        packet[244] = 2; // length
        packet[245..247].copy_from_slice(&7u16.to_be_bytes());
        packet[247] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(dhcp.client_arch(), Some(7));
    }

    #[test]
    fn test_parse_client_uuid() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        // Add client UUID option (Option 97)
        let uuid = [
            0x00, // Type 0 = UUID
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10,
        ];
        packet[243] = option_codes::CLIENT_UUID;
        packet[244] = uuid.len() as u8;
        packet[245..245 + uuid.len()].copy_from_slice(&uuid);
        packet[245 + uuid.len()] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        assert!(dhcp.client_uuid().is_some());
        assert_eq!(dhcp.client_uuid().unwrap().len(), 17);
    }

    #[test]
    fn test_parse_requested_ip() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        packet[243] = option_codes::REQUESTED_IP;
        packet[244] = 4;
        packet[245..249].copy_from_slice(&[192, 168, 1, 50]);
        packet[249] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        let requested = dhcp.options.iter().find_map(|opt| {
            if let DhcpOption::RequestedIp(ip) = opt {
                Some(*ip)
            } else {
                None
            }
        });
        assert_eq!(requested, Some(Ipv4Addr::new(192, 168, 1, 50)));
    }

    #[test]
    fn test_parse_server_identifier() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        packet[243] = option_codes::SERVER_ID;
        packet[244] = 4;
        packet[245..249].copy_from_slice(&[192, 168, 1, 1]);
        packet[249] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        let server_id = dhcp.options.iter().find_map(|opt| {
            if let DhcpOption::ServerIdentifier(ip) = opt {
                Some(*ip)
            } else {
                None
            }
        });
        assert_eq!(server_id, Some(Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn test_parse_with_pad_options() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        // Add PAD options before END
        packet[243] = option_codes::PAD;
        packet[244] = option_codes::PAD;
        packet[245] = option_codes::PAD;
        packet[246] = option_codes::END;

        let result = parser.parse(&packet);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_unknown_option() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        // Add an unknown option (code 200)
        packet[243] = 200;
        packet[244] = 3;
        packet[245..248].copy_from_slice(&[0x01, 0x02, 0x03]);
        packet[248] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        let unknown = dhcp.options.iter().find(|opt| {
            matches!(opt, DhcpOption::Unknown(200, _))
        });
        assert!(unknown.is_some());
    }

    #[test]
    fn test_parse_truncated_option_length() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        // Option with length but insufficient data space
        packet[243] = option_codes::VENDOR_CLASS_ID;
        packet[244] = 100; // Claims 100 bytes

        // Truncate packet so option data is incomplete
        packet.truncate(250); // Only 5 bytes available after length, but claims 100

        let result = parser.parse(&packet);
        assert!(matches!(result, Err(ParseError::InvalidOption { .. })));
    }

    #[test]
    fn test_parse_option_missing_length() {
        let parser = DhcpParser::new();
        let mut packet = vec![0u8; 241]; // Just enough for header + magic + 1 byte
        packet[0] = 1;
        packet[1] = 1;
        packet[2] = 6;
        packet[236..240].copy_from_slice(&DHCP_MAGIC_COOKIE);
        packet[240] = option_codes::VENDOR_CLASS_ID; // No length byte follows

        let result = parser.parse(&packet);
        assert!(matches!(result, Err(ParseError::InvalidOption { .. })));
    }

    #[test]
    fn test_parse_client_id() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        let client_id = [0x01, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]; // Type + MAC
        packet[243] = option_codes::CLIENT_ID;
        packet[244] = client_id.len() as u8;
        packet[245..245 + client_id.len()].copy_from_slice(&client_id);
        packet[245 + client_id.len()] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        let cid = dhcp.options.iter().find_map(|opt| {
            if let DhcpOption::ClientId(data) = opt {
                Some(data.clone())
            } else {
                None
            }
        });
        assert_eq!(cid, Some(client_id.to_vec()));
    }

    #[test]
    fn test_parse_client_ndi() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        let ndi = [0x01, 0x03, 0x10]; // Type, major, minor
        packet[243] = option_codes::CLIENT_NDI;
        packet[244] = ndi.len() as u8;
        packet[245..245 + ndi.len()].copy_from_slice(&ndi);
        packet[245 + ndi.len()] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        let parsed_ndi = dhcp.options.iter().find_map(|opt| {
            if let DhcpOption::ClientNdi(data) = opt {
                Some(data.clone())
            } else {
                None
            }
        });
        assert_eq!(parsed_ndi, Some(ndi.to_vec()));
    }

    #[test]
    fn test_default_impl() {
        let parser = DhcpParser::default();
        let packet = create_test_packet();
        assert!(parser.parse(&packet).is_ok());
    }

    #[test]
    fn test_invalid_message_type_value() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();
        packet[242] = 99; // Invalid message type

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(dhcp.message_type(), None);
    }

    #[test]
    fn test_empty_message_type_option() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();
        packet[241] = 0; // Zero-length message type option
        packet[242] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(dhcp.message_type(), None);
    }

    #[test]
    fn test_short_requested_ip_option() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        packet[243] = option_codes::REQUESTED_IP;
        packet[244] = 2; // Too short (needs 4)
        packet[245..247].copy_from_slice(&[192, 168]);
        packet[247] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        // Option should be skipped due to invalid length
        let requested = dhcp.options.iter().find(|opt| {
            matches!(opt, DhcpOption::RequestedIp(_))
        });
        assert!(requested.is_none());
    }

    #[test]
    fn test_short_client_arch_option() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        packet[243] = option_codes::CLIENT_ARCH;
        packet[244] = 1; // Too short (needs 2)
        packet[245] = 0x07;
        packet[246] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(dhcp.client_arch(), None);
    }

    #[test]
    fn test_invalid_utf8_vendor_class() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        // Invalid UTF-8 sequence
        let invalid_utf8 = [0xff, 0xfe, 0x00, 0x01];
        packet[243] = option_codes::VENDOR_CLASS_ID;
        packet[244] = invalid_utf8.len() as u8;
        packet[245..245 + invalid_utf8.len()].copy_from_slice(&invalid_utf8);
        packet[245 + invalid_utf8.len()] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        // Invalid UTF-8 should result in None
        assert_eq!(dhcp.vendor_class_id(), None);
    }

    #[test]
    fn test_multiple_options() {
        let parser = DhcpParser::new();
        let mut packet = create_test_packet();

        let mut offset = 243;

        // Vendor class
        let vendor = b"PXEClient";
        packet[offset] = option_codes::VENDOR_CLASS_ID;
        packet[offset + 1] = vendor.len() as u8;
        packet[offset + 2..offset + 2 + vendor.len()].copy_from_slice(vendor);
        offset += 2 + vendor.len();

        // Client arch
        packet[offset] = option_codes::CLIENT_ARCH;
        packet[offset + 1] = 2;
        packet[offset + 2..offset + 4].copy_from_slice(&7u16.to_be_bytes());
        offset += 4;

        // Requested IP
        packet[offset] = option_codes::REQUESTED_IP;
        packet[offset + 1] = 4;
        packet[offset + 2..offset + 6].copy_from_slice(&[192, 168, 1, 100]);
        offset += 6;

        packet[offset] = option_codes::END;

        let dhcp = parser.parse(&packet).unwrap();
        assert_eq!(dhcp.vendor_class_id(), Some("PXEClient"));
        assert_eq!(dhcp.client_arch(), Some(7));
        assert!(dhcp.message_type().is_some());
    }
}
