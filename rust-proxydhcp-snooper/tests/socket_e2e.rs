//! Privileged end-to-end check for the raw socket layer.
//!
//! It needs `CAP_NET_RAW` (and `CAP_NET_ADMIN` to create the dummy interface),
//! so it is ignored by default. Build the tests, then run the binary inside a
//! user + network namespace: `unshare -Urn <test-binary> --ignored --nocapture`.
//! See the crate README for the exact commands.

#![cfg(target_os = "linux")]

use std::net::Ipv4Addr;
use std::process::Command;
use std::time::Duration;

use dhcproto::v4::{DhcpOption, MessageType, OptionCode};
use proxydhcp_snooper::proxy::{decode, handle_datagram, OfferConfig};
use proxydhcp_snooper::socket::{interface_index, RawDhcpSocket};

const IFACE: &str = "pxetest0";
const SERVER_IP: &str = "10.9.9.1";
const DISCOVER: &[u8] = include_bytes!("fixtures/discover_bios.bin");

fn ip(args: &[&str]) {
    let status = Command::new("ip")
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("running `ip {}`: {e}", args.join(" ")));
    assert!(status.success(), "`ip {}` failed", args.join(" "));
}

fn setup_interface() {
    // Idempotent: drop any stale interface left by a previous run.
    let _ = Command::new("ip").args(["link", "del", IFACE]).status();
    ip(&["link", "add", IFACE, "type", "dummy"]);
    ip(&["addr", "add", &format!("{SERVER_IP}/24"), "dev", IFACE]);
    ip(&["link", "set", IFACE, "up"]);
}

#[test]
#[ignore = "requires CAP_NET_RAW/CAP_NET_ADMIN; run inside `unshare -Urn`"]
fn captures_real_discover_and_emits_offer() {
    setup_interface();
    let ifindex = interface_index(IFACE).expect("interface exists");
    let server_ip: Ipv4Addr = SERVER_IP.parse().unwrap();
    let cfg = OfferConfig::new(server_ip, 80);

    let snooper = RawDhcpSocket::open(67).expect("open passive DHCP socket (needs CAP_NET_RAW)");
    snooper
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let client = RawDhcpSocket::open(68).expect("open client raw socket");
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    // The client injects the captured DISCOVER: source port 68 -> port 67,
    // forced out the test interface. Nothing is bound to port 67 anywhere.
    client
        .send(DISCOVER, server_ip, 67, ifindex)
        .expect("inject DISCOVER");

    let dg = snooper.recv().expect("snooper captured the DISCOVER");
    assert_eq!(
        dg.ifindex, ifindex,
        "per-packet interface index is preserved"
    );
    assert_eq!(dg.src_port, 68);
    assert_eq!(dg.payload, DISCOVER, "payload is delivered intact");

    // Run the pure policy and send the offer back out the same interface.
    let resp = handle_datagram(&dg.payload, &cfg).expect("offer built");
    snooper
        .send(&resp.bytes, Ipv4Addr::BROADCAST, 68, ifindex)
        .expect("send offer");

    let offer = client.recv().expect("client received the offer");
    assert_eq!(offer.ifindex, ifindex);
    let msg = decode(&offer.payload).expect("offer decodes");
    assert_eq!(msg.opts().msg_type(), Some(MessageType::Offer));
    assert_eq!(
        msg.fname_str().unwrap().unwrap().trim_end_matches('\0'),
        "d0:50:99:4e:05:57/0"
    );
    assert_eq!(
        msg.opts().get(OptionCode::ClassIdentifier),
        Some(&DhcpOption::ClassIdentifier(b"PXEClient".to_vec()))
    );

    println!(
        "captured DISCOVER on ifindex {ifindex}, replied with a {} byte offer",
        offer.payload.len()
    );
}
