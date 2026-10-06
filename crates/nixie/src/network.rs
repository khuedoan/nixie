//! Server address detection and Wake-on-LAN.

use std::net::{Ipv4Addr, UdpSocket};

use anyhow::{bail, Result};
use tracing::debug;

/// True for addresses usable as a general server address: not loopback,
/// not link-local, not unspecified and not broadcast.
fn is_global_unicast(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() && !ip.is_broadcast()
}

fn is_link_local(ip: Ipv4Addr) -> bool {
    ip.octets()[0] == 169 && ip.octets()[1] == 254
}

fn is_loopback(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
}

/// IPv4 addresses of all interfaces as `(ifindex, address)` pairs.
#[cfg(target_os = "linux")]
fn interface_addresses() -> Vec<(u32, Ipv4Addr)> {
    let mut addresses = Vec::new();
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 {
            return addresses;
        }
        let mut current = head;
        while !current.is_null() {
            let ifa = &*current;
            if !ifa.ifa_addr.is_null() && (*ifa.ifa_addr).sa_family as i32 == libc::AF_INET {
                let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                let ip = Ipv4Addr::from(sin.sin_addr.s_addr.to_ne_bytes());
                let index = libc::if_nametoindex(ifa.ifa_name);
                addresses.push((index, ip));
            }
            current = ifa.ifa_next;
        }
        libc::freeifaddrs(head);
    }
    addresses
}

#[cfg(not(target_os = "linux"))]
fn interface_addresses() -> Vec<(u32, Ipv4Addr)> {
    Vec::new()
}

/// Pick the address Nixie should advertise, mirroring Go's `DetectServerAddress`.
pub fn detect_server_address() -> Result<String> {
    let addresses = interface_addresses();
    debug!(?addresses, "interface addresses");

    for predicate in [is_global_unicast, is_link_local] {
        if let Some((_, ip)) = addresses.iter().find(|(_, ip)| predicate(*ip)) {
            return Ok(ip.to_string());
        }
    }

    bail!("no usable unicast address")
}

/// Pick a usable address on one interface, mirroring Pixiecore's `interfaceIP`:
/// global unicast first, then link-local, then loopback.
pub fn interface_ipv4(ifindex: u32) -> Option<Ipv4Addr> {
    let addresses: Vec<Ipv4Addr> = interface_addresses()
        .into_iter()
        .filter(|(index, _)| *index == ifindex)
        .map(|(_, ip)| ip)
        .collect();

    for predicate in [is_global_unicast, is_link_local, is_loopback] {
        if let Some(ip) = addresses.iter().copied().find(|ip| predicate(*ip)) {
            return Some(ip);
        }
    }
    None
}

/// Build a Wake-on-LAN magic packet for `mac`.
pub fn build_magic_packet(mac: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(6 + 16 * mac.len());
    packet.extend_from_slice(&[0xFF; 6]);
    for _ in 0..16 {
        packet.extend_from_slice(mac);
    }
    packet
}

/// Broadcast a Wake-on-LAN magic packet on UDP port 9.
pub fn send_wake_on_lan(mac: &[u8]) -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;
    let packet = build_magic_packet(mac);
    socket.send_to(&packet, (Ipv4Addr::BROADCAST, 9))?;
    Ok(())
}
