//! Netboot components: ProxyDHCP, TFTP and the iPXE boot script.
//!
//! These modules are a Rust port of the Apache-2.0 licensed
//! [`netboot`](https://github.com/danderson/netboot) Pixiecore code that Nixie
//! previously vendored. See `LICENSE` in this directory.

pub mod booter;
pub mod dhcp;
pub mod http;
pub mod tftp;

#[cfg(target_os = "linux")]
pub mod socket;

pub use booter::{Architecture, BootSpec, Booter, Machine};
