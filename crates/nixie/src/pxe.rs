//! The PXE boot server: ProxyDHCP, TFTP, the iPXE HTTP script server and the
//! PXE boot service on port 4011.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use dhcproto::v4::{DhcpOption, Flags, Message, MessageType, Opcode};
use tiny_http::{Header, Response};
use tracing::{debug, info, warn};

use crate::hosts::{HostsConfig, State};
use crate::mac::MacAddr;
use crate::netboot::booter::{Architecture, BootSpec, Booter, Machine};
use crate::netboot::dhcp::{self, Firmware};
use crate::netboot::http::{ipxe_script, query_param};
use crate::netboot::tftp::TftpServer;

pub const PORT_TFTP: u16 = 69;
pub const PORT_HTTP: u16 = 80;
pub const PORT_PXE: u16 = 4011;
pub const PORT_DHCP: u16 = 67;

/// Embedded iPXE x86-64 EFI firmware, the same asset Pixiecore embeds.
pub const IPXE_EFI64: &[u8] = include_bytes!("../assets/ipxe.efi");

/// Boot specifications for Nixie hosts.
pub struct NixieBooter {
    pub address: String,
    pub kernel: String,
    pub initrd: String,
    pub init: String,
    pub hosts_config: HostsConfig,
}

impl Booter for NixieBooter {
    fn boot_spec(&self, machine: Machine) -> Result<Option<BootSpec>> {
        for (flake_output, host) in &self.hosts_config {
            if host.mac_address() == machine.mac {
                debug!(flake_output, "matched boot request to flake output");
                if host.state() != State::Unknown {
                    bail!("PXE boot already used for MAC address: {}", machine.mac);
                }
                return Ok(Some(BootSpec {
                    kernel: "kernel".to_string(),
                    initrd: vec!["initrd".to_string()],
                    cmdline: format!(
                        "init={} loglevel=4 nixie_mac_address={} nixie_api={}:5000",
                        self.init, machine.mac, self.address
                    ),
                    ..BootSpec::default()
                }));
            }
        }
        bail!("unknown MAC address: {}", machine.mac)
    }

    fn read_boot_file(&self, id: &str) -> Result<(File, u64)> {
        let path = match id {
            "kernel" => &self.kernel,
            "initrd" => &self.initrd,
            other => bail!("unknown file ID: {other}"),
        };
        let file = File::open(path).with_context(|| format!("opening boot file {path}"))?;
        let size = file.metadata()?.len();
        Ok((file, size))
    }
}

pub struct PxeServer {
    address: IpAddr,
    booter: Arc<dyn Booter>,
    ipxe: BTreeMap<Firmware, Vec<u8>>,
    shutdown: Arc<AtomicBool>,
    http_server: Mutex<Option<Arc<tiny_http::Server>>>,
}

impl PxeServer {
    pub fn new(
        address: IpAddr,
        booter: Arc<dyn Booter>,
        ipxe: BTreeMap<Firmware, Vec<u8>>,
    ) -> Self {
        Self {
            address,
            booter,
            ipxe,
            shutdown: Arc::new(AtomicBool::new(false)),
            http_server: Mutex::new(None),
        }
    }

    /// Check that the installer files exist before serving.
    pub fn validate_files(paths: &[String]) -> Result<()> {
        for path in paths {
            if !std::path::Path::new(path).exists() {
                bail!("missing installer file: {path}");
            }
        }
        Ok(())
    }

    /// Start serving and block until [`PxeServer::shutdown`] is called or a
    /// component fails.
    pub fn serve(&self) -> Result<()> {
        let http = Arc::new(
            tiny_http::Server::http(SocketAddr::new(self.address, PORT_HTTP))
                .map_err(|e| anyhow::anyhow!("HTTP server: {e}"))?,
        );
        *self.http_server.lock().unwrap() = Some(Arc::clone(&http));

        let tftp_socket = UdpSocket::bind(SocketAddr::new(self.address, PORT_TFTP))
            .context("binding TFTP socket")?;
        let pxe_socket = UdpSocket::bind(SocketAddr::new(self.address, PORT_PXE))
            .context("binding PXE socket")?;
        let dhcp_socket = open_dhcp_socket()?;

        let (error_tx, error_rx) = mpsc::channel();

        {
            let shutdown = Arc::clone(&self.shutdown);
            let server = Arc::clone(&http);
            let booter = Arc::clone(&self.booter);
            spawn(&error_tx, "HTTP", move || {
                serve_http(server, booter, shutdown)
            });
        }
        {
            let shutdown = Arc::clone(&self.shutdown);
            let ipxe = self.ipxe.clone();
            spawn(&error_tx, "TFTP", move || {
                let handler = Arc::new(move |path: &str| -> Result<(Vec<u8>, u64)> {
                    let (mac, firmware) = parse_tftp_path(path)?;
                    let _ = mac;
                    let bytes = ipxe.get(&firmware).cloned().ok_or_else(|| {
                        anyhow::anyhow!("unknown firmware type {}", firmware.number())
                    })?;
                    let size = bytes.len() as u64;
                    Ok((bytes, size))
                });
                TftpServer::new(handler).serve(tftp_socket, shutdown);
                Ok(())
            });
        }
        {
            let shutdown = Arc::clone(&self.shutdown);
            let address = self.address;
            let ipxe = self.ipxe.clone();
            spawn(&error_tx, "PXE", move || {
                serve_pxe(pxe_socket, address, ipxe, shutdown)
            });
        }
        {
            let shutdown = Arc::clone(&self.shutdown);
            let booter = Arc::clone(&self.booter);
            spawn(&error_tx, "DHCP", move || {
                serve_dhcp(dhcp_socket, booter, PORT_HTTP, shutdown)
            });
        }

        while !self.shutdown.load(Ordering::Relaxed) {
            if let Ok(error) = error_rx.try_recv() {
                return Err(error);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(server) = self.http_server.lock().unwrap().as_ref() {
            server.unblock();
        }
    }
}

fn spawn(
    error_tx: &mpsc::Sender<anyhow::Error>,
    name: &'static str,
    task: impl FnOnce() -> Result<()> + Send + 'static,
) {
    let error_tx = error_tx.clone();
    std::thread::spawn(move || {
        if let Err(error) = task() {
            let _ = error_tx.send(error.context(format!("{name} server stopped")));
        }
    });
}

fn open_dhcp_socket() -> Result<crate::netboot::socket::RawDhcpSocket> {
    crate::netboot::socket::RawDhcpSocket::open(PORT_DHCP)
        .context("opening passive DHCP socket (needs CAP_NET_RAW)")
}

// --- HTTP ------------------------------------------------------------------------------------

fn serve_http(
    server: Arc<tiny_http::Server>,
    booter: Arc<dyn Booter>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    for request in server.incoming_requests() {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or("").to_string();
        let result: Result<()> = match path.as_str() {
            "/_/ipxe" => handle_ipxe(request, booter.as_ref(), &url),
            "/_/file" => handle_file(request, booter.as_ref(), &url),
            "/_/booting" => {
                let _ = request.respond(Response::from_string("# Booting"));
                Ok(())
            }
            _ => {
                let _ = request.respond(Response::empty(404));
                Ok(())
            }
        };
        if let Err(error) = result {
            debug!(%error, "error responding to HTTP request");
        }
    }
    Ok(())
}

fn handle_ipxe(request: tiny_http::Request, booter: &dyn Booter, url: &str) -> Result<()> {
    let Some(mac) = query_param(url, "mac") else {
        return respond_text(request, 400, "missing MAC address parameter");
    };
    let Some(arch) = query_param(url, "arch") else {
        return respond_text(request, 400, "missing architecture parameter");
    };
    let mac: MacAddr = match mac.parse() {
        Ok(value) => value,
        Err(_) => return respond_text(request, 400, "invalid MAC address"),
    };
    let arch = match arch.parse::<u8>() {
        Ok(0) => Architecture::IA32,
        Ok(1) => Architecture::X64,
        _ => return respond_text(request, 400, "unknown architecture"),
    };

    let machine = Machine { mac, arch };
    let spec = match booter.boot_spec(machine) {
        Ok(Some(spec)) => spec,
        Ok(None) => return respond_text(request, 404, "you don't netboot"),
        Err(error) => {
            warn!(%error, "couldn't get a boot spec");
            return respond_text(request, 500, "couldn't get a bootspec");
        }
    };

    let host = request
        .headers()
        .iter()
        .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case("host"))
        .map(|header| header.value.as_str().to_string())
        .unwrap_or_else(|| {
            request
                .remote_addr()
                .map(|a| a.to_string())
                .unwrap_or_default()
        });

    let script = match ipxe_script(machine, &spec, &host) {
        Ok(script) => script,
        Err(error) => {
            warn!(%error, "failed to assemble ipxe script");
            return respond_text(request, 500, "couldn't get a boot script");
        }
    };

    info!(addr = ?request.remote_addr(), "sending ipxe boot script");
    let header = Header::from_bytes("Content-Type", "text/plain").unwrap();
    request.respond(Response::from_data(script).with_header(header))?;
    Ok(())
}

fn handle_file(request: tiny_http::Request, booter: &dyn Booter, url: &str) -> Result<()> {
    let Some(name) = query_param(url, "name") else {
        return respond_text(request, 400, "missing filename");
    };
    let (mut file, size) = match booter.read_boot_file(&name) {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, %name, "error getting file");
            return respond_text(request, 500, "couldn't get file");
        }
    };
    let mut data = Vec::with_capacity(size as usize);
    file.read_to_end(&mut data)?;
    let header = Header::from_bytes("Content-Length", size.to_string()).unwrap();
    request.respond(Response::from_data(data).with_header(header))?;
    Ok(())
}

fn respond_text(request: tiny_http::Request, status: u16, message: &str) -> Result<()> {
    request.respond(
        Response::from_string(message)
            .with_status_code(status)
            .with_header(Header::from_bytes("Content-Type", "text/plain").unwrap()),
    )?;
    Ok(())
}

// --- TFTP path parsing -----------------------------------------------------------------------

fn parse_tftp_path(path: &str) -> Result<(MacAddr, Firmware)> {
    let (mac, firmware) = path
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("unknown path {path:?}"))?;
    let mac: MacAddr = mac
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid MAC address {mac:?}"))?;
    let firmware = match firmware.parse::<u8>() {
        Ok(0) => Firmware::X86PC,
        Ok(1) => Firmware::EFI32,
        Ok(2) => Firmware::EFI64,
        Ok(3) => Firmware::EFIBC,
        Ok(4) => Firmware::X86Ipxe,
        Ok(5) => Firmware::PixiecoreIpxe,
        _ => bail!("unknown firmware type {firmware:?}"),
    };
    Ok((mac, firmware))
}

// --- ProxyDHCP -------------------------------------------------------------------------------

fn serve_dhcp(
    socket: crate::netboot::socket::RawDhcpSocket,
    booter: Arc<dyn Booter>,
    http_port: u16,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
    while !shutdown.load(Ordering::Relaxed) {
        let datagram = match socket.recv() {
            Ok(datagram) => datagram,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        let request = match dhcp::decode(&datagram.payload) {
            Ok(request) => request,
            Err(error) => {
                debug!(%error, "ignoring non-DHCP packet");
                continue;
            }
        };
        if let Err(error) = dhcp::is_boot_dhcp(&request) {
            debug!(%error, "ignoring packet");
            continue;
        }
        let classification = match dhcp::validate_dhcp(&request) {
            Ok(classification) => classification,
            Err(error) => {
                info!(%error, "unusable packet");
                continue;
            }
        };
        if let Err(error) = booter.boot_spec(classification.machine) {
            debug!(%error, "no boot spec for machine, ignoring boot request");
            continue;
        }
        let Some(server_ip) = crate::network::interface_ipv4(datagram.ifindex) else {
            warn!(ifindex = datagram.ifindex, "no source address on interface");
            continue;
        };

        let offer = dhcp::offer_dhcp(&request, &classification, server_ip, http_port);
        let bytes = match dhcp::encode(&offer) {
            Ok(bytes) => bytes,
            Err(error) => {
                warn!(%error, "failed to construct ProxyDHCP offer");
                continue;
            }
        };
        info!(
            mac = %classification.machine.mac,
            firmware = ?classification.firmware,
            "offering to boot"
        );
        if let Err(error) = socket.send(&bytes, Ipv4Addr::BROADCAST, 68, datagram.ifindex) {
            warn!(%error, "failed to send ProxyDHCP offer");
        }
    }
    Ok(())
}

// --- PXE boot service (port 4011) ------------------------------------------------------------

fn serve_pxe(
    socket: UdpSocket,
    address: IpAddr,
    ipxe: BTreeMap<Firmware, Vec<u8>>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
    let server_ip = match address {
        IpAddr::V4(ip) => ip,
        IpAddr::V6(_) => bail!("IPv6 is not supported yet"),
    };
    let mut buf = vec![0u8; 1024];
    while !shutdown.load(Ordering::Relaxed) {
        let (n, peer) = match socket.recv_from(&mut buf) {
            Ok(value) => value,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        let packet = match dhcp::decode(&buf[..n]) {
            Ok(packet) => packet,
            Err(error) => {
                debug!(%error, "packet is not a DHCP packet");
                continue;
            }
        };
        if let Err(error) = dhcp::is_boot_dhcp(&packet) {
            debug!(%error, "ignoring packet on PXE port");
        }
        let firmware = match validate_pxe(&packet, &ipxe) {
            Ok(firmware) => firmware,
            Err(error) => {
                info!(%error, "unusable PXE packet");
                continue;
            }
        };

        let response = offer_pxe(&packet, server_ip, firmware);
        let bytes = match dhcp::encode(&response) {
            Ok(bytes) => bytes,
            Err(error) => {
                warn!(%error, "failed to marshal PXE offer");
                continue;
            }
        };
        if let Err(error) = socket.send_to(&bytes, peer) {
            warn!(%error, %peer, "failed to send PXE response");
        }
    }
    Ok(())
}

fn validate_pxe(packet: &Message, ipxe: &BTreeMap<Firmware, Vec<u8>>) -> Result<Firmware> {
    if packet.hlen() != 6 {
        bail!("unsupported hardware address length {}", packet.hlen());
    }
    let arch = match packet.opts().get(dhcp::OPTION_CLIENT_ARCH) {
        Some(DhcpOption::ClientSystemArchitecture(arch)) => u16::from(*arch),
        _ => bail!("malformed DHCP option 93 (required for PXE)"),
    };
    let firmware = match arch {
        6 => Firmware::EFI32,
        7 => Firmware::EFI64,
        9 => Firmware::EFIBC,
        other => bail!("unsupported client firmware type '{other}'"),
    };
    if !ipxe.contains_key(&firmware) {
        bail!("unsupported client firmware type '{arch}'");
    }

    if let Some(DhcpOption::ClientMachineIdentifier(guid)) =
        packet.opts().get(dhcp::OPTION_CLIENT_GUID)
    {
        match guid.len() {
            0 => {}
            17 if guid[0] == 0 => {}
            17 => bail!("malformed client GUID (option 97), leading byte must be zero"),
            _ => bail!("malformed client GUID (option 97), wrong size"),
        }
    }

    Ok(firmware)
}

fn offer_pxe(packet: &Message, server_ip: Ipv4Addr, firmware: Firmware) -> Message {
    let mut response = Message::default();
    response.set_opcode(Opcode::BootReply);
    response.set_xid(packet.xid());
    response.set_flags(Flags::default().set_broadcast());
    response.set_chaddr(packet.chaddr());
    response.set_ciaddr(packet.ciaddr());
    response.set_giaddr(packet.giaddr());
    response.set_siaddr(server_ip);
    response.set_sname_str(server_ip.to_string());
    response.set_fname_str(format!("{}/{}", mac_from_chaddr(packet), firmware.number()));

    let options = response.opts_mut();
    options.insert(DhcpOption::MessageType(MessageType::Ack));
    options.insert(DhcpOption::ServerIdentifier(server_ip));
    options.insert(DhcpOption::ClassIdentifier(b"PXEClient".to_vec()));
    if let Some(DhcpOption::ClientMachineIdentifier(guid)) =
        packet.opts().get(dhcp::OPTION_CLIENT_GUID)
    {
        options.insert(DhcpOption::ClientMachineIdentifier(guid.clone()));
    }

    response
}

fn mac_from_chaddr(packet: &Message) -> MacAddr {
    let c = packet.chaddr();
    MacAddr::new([c[0], c[1], c[2], c[3], c[4], c[5]])
}
