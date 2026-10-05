//! A prototype passive ProxyDHCP/PXE snooper for Linux.
//!
//! The wire format is handled by [`dhcproto`]; the Nixie-specific firmware
//! classification and offer construction live in [`proxy`] as pure functions.
//! The privileged raw-socket layer lives in [`socket`] and is only compiled on
//! Linux.

pub mod proxy;

#[cfg(target_os = "linux")]
pub mod socket;

pub use proxy::{
    decode, encode, handle_datagram, is_boot_dhcp, offer_dhcp, validate_dhcp, Architecture,
    BootError, Classification, Firmware, MacAddr, Machine, OfferConfig, ProxyError, ProxyResponse,
    PxeOptions,
};
