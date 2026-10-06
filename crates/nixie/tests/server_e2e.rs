//! End-to-end server test: HTTP boot script, TFTP, ProxyDHCP and the API.
//!
//! Needs `CAP_NET_RAW` and `CAP_NET_ADMIN`, so it is ignored by default. Build
//! the tests, then run this binary inside a user + network namespace:
//!
//!     cargo test --no-run
//!     unshare -Urn sh -c 'ip link set lo up; target/debug/deps/server_e2e-* --ignored --nocapture'

#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::process::Command;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use nixie::api::{start_api_server, Api, ApiConfig};
use nixie::hosts;
use nixie::netboot::dhcp::Firmware;
use nixie::netboot::socket::{interface_index, RawDhcpSocket};
use nixie::pxe::{NixieBooter, PxeServer};

const IFACE: &str = "pxetest0";
const SERVER_IP: &str = "10.9.9.1";
const MAC: &str = "d0:50:99:4e:05:57";
const DISCOVER: &[u8] = include_bytes!("fixtures/discover_bios.bin");
const IPXE_PAYLOAD: &[u8] = b"ipxe-test-firmware";

fn ip(args: &[&str]) {
    let status = Command::new("ip")
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("running `ip {}`: {e}", args.join(" ")));
    assert!(status.success(), "`ip {}` failed", args.join(" "));
}

fn setup_interface() {
    let _ = Command::new("ip").args(["link", "del", IFACE]).status();
    ip(&["link", "add", IFACE, "type", "dummy"]);
    ip(&["addr", "add", &format!("{SERVER_IP}/24"), "dev", IFACE]);
    ip(&["link", "set", IFACE, "up"]);
}

fn workdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nixie-server-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_installer_files(dir: &std::path::Path) -> (String, String, String) {
    let kernel = dir.join("bzImage");
    let initrd = dir.join("initrd");
    let init = dir.join("init");
    std::fs::write(&kernel, b"kernel-bytes").unwrap();
    std::fs::write(&initrd, b"initrd-bytes").unwrap();
    std::fs::write(&init, b"init-bytes").unwrap();
    (
        kernel.to_string_lossy().into_owned(),
        initrd.to_string_lossy().into_owned(),
        init.to_string_lossy().into_owned(),
    )
}

fn http_get(addr: SocketAddr, path: &str, host: &str) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();

    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("header terminator");
    let head = String::from_utf8_lossy(&response[..split]).into_owned();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("status code");
    (status, response[split + 4..].to_vec())
}

fn wait_for_http(addr: SocketAddr) {
    for _ in 0..50 {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("HTTP server did not come up on {addr}");
}

fn tftp_fetch(server: &str, port: u16, path: &str) -> Vec<u8> {
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut rrq = vec![0u8, 1];
    rrq.extend_from_slice(path.as_bytes());
    rrq.push(0);
    rrq.extend_from_slice(b"octet\0");
    client.send_to(&rrq, (server, port)).unwrap();

    let mut buf = [0u8; 4096];
    let (n, from) = client.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..2], &[0, 3], "TFTP data packet");
    assert_eq!(&buf[2..4], &[0, 1], "first block");
    let payload = buf[4..n].to_vec();
    let _ = client.send_to(&[0, 4, 0, 1], from);
    payload
}

#[test]
#[ignore = "requires CAP_NET_RAW/CAP_NET_ADMIN; run inside `unshare -Urn`"]
fn serves_boot_script_tftp_dhcp_and_api() {
    // Set OTEL_EXPORTER_OTLP_ENDPOINT to also exercise span export; without it
    // this only installs stderr logging.
    nixie::otel::init(false);
    setup_interface();
    let ifindex = interface_index(IFACE).expect("interface exists");
    let dir = workdir();
    let (kernel, initrd, init) = write_installer_files(&dir);

    let hosts_path = dir.join("hosts.json");
    std::fs::write(
        &hosts_path,
        format!(r#"{{"machine1": {{"mac_address": "{MAC}"}}}}"#),
    )
    .unwrap();
    let hosts_path = hosts_path.to_string_lossy().into_owned();
    let hosts_config = hosts::load_hosts_config(&hosts_path).unwrap();

    let booter = NixieBooter {
        address: SERVER_IP.to_string(),
        kernel,
        initrd,
        init,
        hosts_config: hosts_config.clone(),
    };
    let mut ipxe = BTreeMap::new();
    ipxe.insert(Firmware::EFI64, IPXE_PAYLOAD.to_vec());
    let server = Arc::new(PxeServer::new(
        IpAddr::V4(SERVER_IP.parse().unwrap()),
        Arc::new(booter),
        ipxe,
    ));

    let serve_handle = {
        let server = Arc::clone(&server);
        std::thread::spawn(move || server.serve())
    };

    let http_addr: SocketAddr = format!("{SERVER_IP}:80").parse().unwrap();
    wait_for_http(http_addr);

    // HTTP: the iPXE boot script.
    let (status, body) = http_get(
        http_addr,
        &format!("/_/ipxe?mac={MAC}&arch=1"),
        &format!("{SERVER_IP}:80"),
    );
    assert_eq!(status, 200);
    let script = String::from_utf8(body).unwrap();
    assert!(script.starts_with("#!ipxe\n"), "script: {script}");
    assert!(
        script.contains("kernel --name kernel http://"),
        "script: {script}"
    );
    assert!(
        script.contains(&format!("nixie_mac_address={MAC}")),
        "script: {script}"
    );
    assert!(
        script.contains(&format!("nixie_api={SERVER_IP}:5000")),
        "script: {script}"
    );

    // HTTP: unknown MAC is refused.
    let (status, _) = http_get(
        http_addr,
        "/_/ipxe?mac=00:00:00:00:00:00&arch=1",
        &format!("{SERVER_IP}:80"),
    );
    assert_eq!(status, 500);

    // HTTP: the kernel file.
    let (status, body) = http_get(http_addr, "/_/file?name=kernel", &format!("{SERVER_IP}:80"));
    assert_eq!(status, 200);
    assert_eq!(body, b"kernel-bytes");

    // TFTP: the iPXE firmware for the firmware type in the boot filename.
    let payload = tftp_fetch(SERVER_IP, 69, &format!("{MAC}/2"));
    assert_eq!(payload, IPXE_PAYLOAD);

    // ProxyDHCP: inject the captured DISCOVER, expect an offer on port 68.
    let client = RawDhcpSocket::open(68).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client
        .send(DISCOVER, SERVER_IP.parse().unwrap(), 67, ifindex)
        .unwrap();
    let offer = client.recv().expect("received ProxyDHCP offer");
    assert_eq!(offer.ifindex, ifindex);
    let message = nixie::netboot::dhcp::decode(&offer.payload).unwrap();
    assert_eq!(
        message.opts().msg_type(),
        Some(dhcproto::v4::MessageType::Offer)
    );
    assert_eq!(
        message.fname_str().unwrap().unwrap().trim_end_matches('\0'),
        format!("{MAC}/0")
    );

    // API: ping and an unknown install request.
    let (done_tx, _done_rx) = mpsc::channel();
    let api = Api::new(ApiConfig {
        hosts_config: hosts_config.clone(),
        hosts_file: hosts_path,
        flake: ".".to_string(),
        install_ssh_key: String::new(),
        deployment_ssh_user: "root".to_string(),
        deployment_ssh_key: String::new(),
        ssh_agent_socket: "/run/ssh-agent.sock".to_string(),
        debug: false,
        done_tx,
    });
    std::thread::spawn(move || {
        let _ = start_api_server(api);
    });
    let api_addr: SocketAddr = format!("{SERVER_IP}:5000").parse().unwrap();
    wait_for_http(api_addr);

    let (status, body) = http_get(api_addr, "/ping", &format!("{SERVER_IP}:5000"));
    assert_eq!(status, 200);
    assert_eq!(body, b"pong");

    // POST /install with an unknown MAC must not start an install.
    let mut stream = TcpStream::connect_timeout(&api_addr, Duration::from_secs(2)).unwrap();
    let payload = br#"{"mac_address":"00:00:00:00:00:00"}"#;
    write!(
        stream,
        "POST /install HTTP/1.1\r\nHost: {SERVER_IP}:5000\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    )
    .unwrap();
    stream.write_all(payload).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let head = String::from_utf8_lossy(&response);
    assert!(head.starts_with("HTTP/1.1 404"), "response: {head}");

    server.shutdown();
    let _ = serve_handle.join();
    // Flush exported spans before the test process exits.
    nixie::otel::shutdown();
}
