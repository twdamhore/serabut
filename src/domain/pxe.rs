//! PXE-specific domain models.

use std::fmt;

/// PXE client system architecture types as defined in RFC 4578.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PxeClientArch {
    IntelX86Bios,
    NecPc98,
    Efi386,
    EfiBC,
    EfiX64,
    EfiArm32,
    EfiArm64,
    Unknown(u16),
}

impl PxeClientArch {
    pub fn from_u16(value: u16) -> Self {
        match value {
            0 => Self::IntelX86Bios,
            1 => Self::NecPc98,
            2 => Self::Efi386,
            6 => Self::EfiBC,
            7 => Self::EfiX64,
            9 => Self::EfiArm32,
            11 => Self::EfiArm64,
            other => Self::Unknown(other),
        }
    }

    pub fn is_efi(&self) -> bool {
        matches!(
            self,
            Self::Efi386 | Self::EfiBC | Self::EfiX64 | Self::EfiArm32 | Self::EfiArm64
        )
    }

    pub fn is_bios(&self) -> bool {
        matches!(self, Self::IntelX86Bios)
    }
}

impl fmt::Display for PxeClientArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IntelX86Bios => write!(f, "x86 BIOS"),
            Self::NecPc98 => write!(f, "NEC/PC98"),
            Self::Efi386 => write!(f, "EFI x86"),
            Self::EfiBC => write!(f, "EFI BC"),
            Self::EfiX64 => write!(f, "EFI x64"),
            Self::EfiArm32 => write!(f, "EFI ARM32"),
            Self::EfiArm64 => write!(f, "EFI ARM64"),
            Self::Unknown(code) => write!(f, "Unknown({code})"),
        }
    }
}

/// Parsed PXE information from a DHCP packet.
#[derive(Debug, Clone)]
pub struct PxeInfo {
    /// The vendor class identifier string (e.g., "PXEClient:Arch:00000:UNDI:002001")
    pub vendor_class: String,
    /// Parsed client architecture
    pub architecture: Option<PxeClientArch>,
    /// Client UUID if present
    pub uuid: Option<String>,
}

impl PxeInfo {
    /// Parse PXE info from vendor class identifier string.
    pub fn from_vendor_class(vendor_class: &str) -> Option<Self> {
        if !vendor_class.starts_with("PXEClient") {
            return None;
        }

        // Parse architecture from vendor class string if present
        // Format: PXEClient:Arch:XXXXX:UNDI:YYYYYY
        let architecture = Self::parse_arch_from_vendor_class(vendor_class);

        Some(Self {
            vendor_class: vendor_class.to_string(),
            architecture,
            uuid: None,
        })
    }

    fn parse_arch_from_vendor_class(vendor_class: &str) -> Option<PxeClientArch> {
        // Look for "Arch:XXXXX" pattern
        let arch_prefix = "Arch:";
        let arch_start = vendor_class.find(arch_prefix)?;
        let arch_value_start = arch_start + arch_prefix.len();

        // Extract the numeric part
        let remaining = &vendor_class[arch_value_start..];
        let arch_end = remaining
            .find(':')
            .unwrap_or(remaining.len())
            .min(remaining.find(' ').unwrap_or(remaining.len()));

        let arch_str = &remaining[..arch_end];
        let arch_value: u16 = arch_str.parse().ok()?;

        Some(PxeClientArch::from_u16(arch_value))
    }

    /// Set the architecture from Option 93 if available.
    pub fn with_architecture(mut self, arch: u16) -> Self {
        self.architecture = Some(PxeClientArch::from_u16(arch));
        self
    }

    /// Set the UUID if available.
    pub fn with_uuid(mut self, uuid: &[u8]) -> Self {
        if uuid.len() >= 17 && uuid[0] == 0 {
            // Type 0 = UUID/GUID, skip the type byte
            self.uuid = Some(format_uuid(&uuid[1..17]));
        } else if uuid.len() >= 16 {
            self.uuid = Some(format_uuid(&uuid[..16]));
        }
        self
    }
}

/// Format a 16-byte UUID as a string.
fn format_uuid(bytes: &[u8]) -> String {
    if bytes.len() < 16 {
        return hex::encode(bytes);
    }

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

// Simple hex encoding since we don't want to add another dependency
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
