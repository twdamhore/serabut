//! Domain models for DHCP PXE boot listening.
//!
//! This module contains the core domain types that are independent
//! of any infrastructure concerns (SRP, DIP).

mod dhcp;
mod events;
mod pxe;

pub use dhcp::{DhcpMessageType, DhcpOption, DhcpPacket};
pub use events::PxeBootEvent;
pub use pxe::{PxeClientArch, PxeInfo};
