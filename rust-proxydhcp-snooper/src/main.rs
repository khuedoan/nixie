//! Prototype ProxyDHCP/PXE snooper.
//!
//! This binary opens the passive raw socket and runs the proxy policy. The
//! wire format is `dhcproto`'s job, and the policy is in the library so it can
//! be tested without privileges.

use std::process::ExitCode;

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    use std::env;
    use std::net::Ipv4Addr;

    use proxydhcp_snooper::proxy::{handle_datagram, OfferConfig};
    use proxydhcp_snooper::socket::RawDhcpSocket;

    let args: Vec<String> = env::args().collect();
    let Some(server_ip) = args.get(1) else {
        eprintln!("usage: {} <server-ip> [http-port]", args[0]);
        return ExitCode::FAILURE;
    };
    let server_ip: Ipv4Addr = match server_ip.parse() {
        Ok(ip) => ip,
        Err(e) => {
            eprintln!("invalid server IP: {e}");
            return ExitCode::FAILURE;
        }
    };
    let http_port: u16 = match args.get(2) {
        Some(p) => p.parse().unwrap_or(80),
        None => 80,
    };

    let cfg = OfferConfig::new(server_ip, http_port);
    let sock = match RawDhcpSocket::open(67) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("opening passive DHCP socket on port 67 (needs CAP_NET_RAW): {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("listening for PXE DHCPDISCOVER on 0.0.0.0:67 without binding it; server {server_ip}");

    loop {
        let dg = match sock.recv() {
            Ok(dg) => dg,
            Err(e) => {
                eprintln!("recv: {e}");
                return ExitCode::FAILURE;
            }
        };
        match handle_datagram(&dg.payload, &cfg) {
            Ok(resp) => {
                println!(
                    "offering to boot {} ({:?}) from {} on ifindex {}",
                    resp.classification.machine.mac,
                    resp.classification.firmware,
                    dg.src,
                    dg.ifindex
                );
                if let Err(e) = sock.send(&resp.bytes, Ipv4Addr::BROADCAST, 68, dg.ifindex) {
                    eprintln!("send: {e}");
                }
            }
            Err(e) => eprintln!(
                "ignoring packet from {} (ifindex {}): {e}",
                dg.src, dg.ifindex
            ),
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("the passive raw socket is only implemented on Linux");
    ExitCode::FAILURE
}
