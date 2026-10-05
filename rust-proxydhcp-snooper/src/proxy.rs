//! Pure ProxyDHCP classification and offer construction.
//!
//! This mirrors the Nixie-specific parts of the Go pixiecore fork:
//! `isBootDHCP`, `validateDHCP` and `offerDHCP` in
//! `internal/netboot/pixiecore/dhcp.go`. The DHCPv4 wire format is delegated
//! to `dhcproto`; everything here is a pure function of decoded messages so it
//! can be unit tested without any privileges.

use std::error::Error;
use std::fmt;
use std::net::Ipv4Addr;

use dhcproto::error::{DecodeError, EncodeError};
use dhcproto::v4::{DhcpOption, Flags, Message, MessageType, Opcode, OptionCode};
use dhcproto::{Decodable, Decoder, Encodable, Encoder};

/// Option 93: client system architecture (PXE).
pub const OPTION_CLIENT_ARCH: OptionCode = OptionCode::ClientSystemArchitecture;
/// Option 97: client machine identifier (GUID). PXE requires a leading zero byte.
pub const OPTION_CLIENT_GUID: OptionCode = OptionCode::ClientMachineIdentifier;
/// Option 60: vendor class identifier ("PXEClient:Arch:...").
pub const OPTION_VENDOR_CLASS: OptionCode = OptionCode::ClassIdentifier;
/// Option 77: user class ("iPXE", "pixiecore", ...).
pub const OPTION_USER_CLASS: OptionCode = OptionCode::UserClass;
/// Option 54: server identifier, echoed in the offer.
pub const OPTION_SERVER_IDENTIFIER: OptionCode = OptionCode::ServerIdentifier;
/// Option 43: vendor-specific options; holds the encapsulated PXE options.
pub const OPTION_VENDOR_EXTENSIONS: OptionCode = OptionCode::VendorExtensions;

/// The PXE-relevant options extracted from a decoded DISCOVER.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PxeOptions {
    /// Raw two-byte value of option 93, if present.
    pub client_arch: Option<u16>,
    /// Raw value of option 97, if present. The leading byte must be zero.
    pub client_guid: Option<Vec<u8>>,
    /// Raw value of option 60, if present.
    pub vendor_class: Option<Vec<u8>>,
    /// Raw value of option 77, if present.
    pub user_class: Option<Vec<u8>>,
}

impl PxeOptions {
    /// Extract options 93, 97, 60 and 77 from an already decoded message.
    pub fn from_message(msg: &Message) -> Self {
        let opts = msg.opts();
        let client_arch = match opts.get(OPTION_CLIENT_ARCH) {
            Some(DhcpOption::ClientSystemArchitecture(arch)) => Some(u16::from(*arch)),
            _ => None,
        };
        Self {
            client_arch,
            client_guid: vec_option(opts.get(OPTION_CLIENT_GUID)),
            vendor_class: vec_option(opts.get(OPTION_VENDOR_CLASS)),
            user_class: vec_option(opts.get(OPTION_USER_CLASS)),
        }
    }
}

fn vec_option(opt: Option<&DhcpOption>) -> Option<Vec<u8>> {
    match opt {
        Some(DhcpOption::ClientMachineIdentifier(v)) => Some(v.clone()),
        Some(DhcpOption::ClassIdentifier(v)) => Some(v.clone()),
        Some(DhcpOption::UserClass(v)) => Some(v.clone()),
        _ => None,
    }
}

/// CPU architecture reported to booters. Mirrors Go's `Architecture`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    IA32,
    X64,
}

impl Architecture {
    /// Numeric value used in the iPXE boot script URL, matching Go's iota.
    pub fn number(self) -> u8 {
        match self {
            Architecture::IA32 => 0,
            Architecture::X64 => 1,
        }
    }
}

/// Firmware type. The discriminants match Go's `Firmware` iota because the
/// numeric value is embedded in the boot filename.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Firmware {
    X86PC = 0,
    EFI32 = 1,
    EFI64 = 2,
    EFIBC = 3,
    X86Ipxe = 4,
    PixiecoreIpxe = 5,
}

impl Firmware {
    /// Numeric value as used in the boot filename (`<mac>/<number>`).
    pub fn number(self) -> u8 {
        self as u8
    }
}

/// An Ethernet hardware address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}

/// A machine that wants to boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Machine {
    pub mac: MacAddr,
    pub arch: Architecture,
}

/// The result of validating a boot request: which machine and which firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    pub machine: Machine,
    pub firmware: Firmware,
}

/// Why a packet is not a usable PXE boot request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootError {
    /// The BOOTP/DHCP message type is not DHCPDISCOVER.
    NotDiscover,
    /// The packet carries no DHCP message type option.
    MissingMessageType,
    /// Option 93 is required for PXE and was absent.
    MissingClientArch,
    /// Option 93 held a firmware type we do not support.
    UnsupportedClientArch(u16),
    /// The chaddr length is not 6 (Ethernet).
    BadHardwareAddrLength(u8),
    /// Option 97 was present but not 17 bytes long.
    BadGuidLength(usize),
    /// Option 97 was 17 bytes but its leading byte was non-zero.
    BadGuidLeadByte(u8),
}

impl fmt::Display for BootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BootError::NotDiscover => write!(f, "packet is not a DHCPDISCOVER"),
            BootError::MissingMessageType => write!(f, "packet has no DHCP message type"),
            BootError::MissingClientArch => {
                write!(f, "not a PXE boot request (missing option 93)")
            }
            BootError::UnsupportedClientArch(a) => {
                write!(f, "unsupported client firmware type '{a}'")
            }
            BootError::BadHardwareAddrLength(n) => {
                write!(f, "unsupported hardware address length {n}")
            }
            BootError::BadGuidLength(n) => {
                write!(f, "malformed client GUID (option 97), wrong size {n}")
            }
            BootError::BadGuidLeadByte(b) => write!(
                f,
                "malformed client GUID (option 97), leading byte must be zero (got {b})"
            ),
        }
    }
}

impl Error for BootError {}

/// Errors from the end-to-end `handle_datagram` policy path.
#[derive(Debug)]
pub enum ProxyError {
    Decode(DecodeError),
    Boot(BootError),
    Encode(EncodeError),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyError::Decode(e) => write!(f, "decoding DHCP packet: {e}"),
            ProxyError::Boot(e) => write!(f, "{e}"),
            ProxyError::Encode(e) => write!(f, "encoding DHCP offer: {e}"),
        }
    }
}

impl Error for ProxyError {}

impl From<DecodeError> for ProxyError {
    fn from(e: DecodeError) -> Self {
        ProxyError::Decode(e)
    }
}

impl From<BootError> for ProxyError {
    fn from(e: BootError) -> Self {
        ProxyError::Boot(e)
    }
}

impl From<EncodeError> for ProxyError {
    fn from(e: EncodeError) -> Self {
        ProxyError::Encode(e)
    }
}

/// Decode a DHCPv4 message from its UDP payload.
pub fn decode(bytes: &[u8]) -> Result<Message, DecodeError> {
    Message::decode(&mut Decoder::new(bytes))
}

/// Encode a DHCPv4 message to its UDP payload.
pub fn encode(msg: &Message) -> Result<Vec<u8>, EncodeError> {
    let mut buf = Vec::new();
    let mut enc = Encoder::new(&mut buf);
    msg.encode(&mut enc)?;
    Ok(buf)
}

/// Mirrors Go's `isBootDHCP`: only DISCOVERs that carry option 93 are boot requests.
pub fn is_boot_dhcp(msg: &Message) -> Result<(), BootError> {
    match msg.opts().msg_type() {
        Some(MessageType::Discover) => {}
        Some(_) => return Err(BootError::NotDiscover),
        None => return Err(BootError::MissingMessageType),
    }
    if msg.opts().get(OPTION_CLIENT_ARCH).is_none() {
        return Err(BootError::MissingClientArch);
    }
    Ok(())
}

/// Mirrors Go's `validateDHCP`: classify firmware from options 93, 77 and 97.
pub fn validate_dhcp(msg: &Message) -> Result<Classification, BootError> {
    let pxe = PxeOptions::from_message(msg);

    let arch_opt = pxe.client_arch.ok_or(BootError::MissingClientArch)?;
    let (arch, mut firmware) = match arch_opt {
        0 => (Architecture::IA32, Firmware::X86PC),
        6 => (Architecture::IA32, Firmware::EFI32),
        7 => (Architecture::X64, Firmware::EFI64),
        9 => (Architecture::X64, Firmware::EFIBC),
        other => return Err(BootError::UnsupportedClientArch(other)),
    };

    // Sub-breeds identified by the user-class option. These only change the
    // firmware type, not the architecture reported to booters.
    if let Some(user_class) = pxe.user_class.as_deref() {
        if user_class == b"iPXE" && firmware == Firmware::X86PC {
            firmware = Firmware::X86Ipxe;
        }
        if user_class == b"pixiecore" {
            firmware = Firmware::PixiecoreIpxe;
        }
    }

    // A missing GUID is a spec violation but real PXE ROMs do it; accept it.
    match pxe.client_guid.as_deref() {
        None | Some([]) => {}
        Some(guid) if guid.len() == 17 && guid[0] == 0 => {}
        Some(guid) if guid.len() == 17 => return Err(BootError::BadGuidLeadByte(guid[0])),
        Some(guid) => return Err(BootError::BadGuidLength(guid.len())),
    }

    let hlen = msg.hlen();
    if hlen != 6 {
        return Err(BootError::BadHardwareAddrLength(hlen));
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(msg.chaddr());

    Ok(Classification {
        machine: Machine {
            mac: MacAddr(mac),
            arch,
        },
        firmware,
    })
}

/// Static configuration needed to build an offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfferConfig {
    /// Address of the interface the request arrived on.
    pub server_ip: Ipv4Addr,
    /// HTTP port used when chainloading to the full iPXE.
    pub http_port: u16,
}

impl OfferConfig {
    pub fn new(server_ip: Ipv4Addr, http_port: u16) -> Self {
        Self {
            server_ip,
            http_port,
        }
    }
}

/// Mirrors Go's `offerDHCP`: build the ProxyDHCP offer for a request.
///
/// The Go implementation can only fail on its default firmware branch, which
/// is unreachable because [`Firmware`] is a closed enum, so this is infallible.
pub fn offer_dhcp(request: &Message, class: &Classification, cfg: &OfferConfig) -> Message {
    let server_ip = cfg.server_ip;
    let mut resp = Message::default();
    resp.set_opcode(Opcode::BootReply);
    resp.set_xid(request.xid());
    resp.set_flags(Flags::default().set_broadcast());
    resp.set_chaddr(class.machine.mac.as_bytes());
    resp.set_giaddr(request.giaddr());
    resp.set_siaddr(server_ip);

    {
        let opts = resp.opts_mut();
        opts.insert(DhcpOption::MessageType(MessageType::Offer));
        opts.insert(DhcpOption::ServerIdentifier(server_ip));
        // The server identifies itself as a PXEClient, though it is a server.
        opts.insert(DhcpOption::ClassIdentifier(b"PXEClient".to_vec()));
        if let Some(DhcpOption::ClientMachineIdentifier(guid)) =
            request.opts().get(OPTION_CLIENT_GUID)
        {
            opts.insert(DhcpOption::ClientMachineIdentifier(guid.clone()));
        }
    }

    // PXE Boot Server Discovery Control - bypass, boot straight from filename.
    let bypass_boot_server_discovery = || vec![6, 1, 8, 255];

    match class.firmware {
        Firmware::X86PC => {
            resp.opts_mut()
                .insert(DhcpOption::VendorExtensions(bypass_boot_server_discovery()));
            resp.set_sname_str(server_ip.to_string());
            resp.set_fname_str(format!("{}/{}", class.machine.mac, class.firmware.number()));
        }
        Firmware::X86Ipxe => {
            resp.opts_mut()
                .insert(DhcpOption::VendorExtensions(bypass_boot_server_discovery()));
            resp.set_fname_str(format!(
                "tftp://{server_ip}/{}/{}",
                class.machine.mac,
                class.firmware.number()
            ));
        }
        Firmware::EFI32 | Firmware::EFI64 | Firmware::EFIBC => {
            // Some UEFI firmwares ignore offers that bypass boot server
            // discovery, but support the BINL-style variant where option 43 is
            // omitted and they call back on port 4011.
            resp.set_sname_str(server_ip.to_string());
            resp.set_fname_str(format!("{}/{}", class.machine.mac, class.firmware.number()));
        }
        Firmware::PixiecoreIpxe => {
            resp.set_fname_str(format!(
                "http://{server_ip}:{}/_/ipxe?arch={}&mac={}",
                cfg.http_port,
                class.machine.arch.number(),
                class.machine.mac
            ));
        }
    }

    resp
}

/// A complete ProxyDHCP response ready to be sent back on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyResponse {
    pub bytes: Vec<u8>,
    pub classification: Classification,
}

/// The full proxy policy: decode, filter, classify and build an offer.
///
/// This is the pure equivalent of the Go `serveDHCP` loop body, minus the
/// socket I/O (which supplies the per-packet interface).
pub fn handle_datagram(raw: &[u8], cfg: &OfferConfig) -> Result<ProxyResponse, ProxyError> {
    let request = decode(raw)?;
    is_boot_dhcp(&request)?;
    let classification = validate_dhcp(&request)?;
    let offer = offer_dhcp(&request, &classification, cfg);
    Ok(ProxyResponse {
        bytes: encode(&offer)?,
        classification,
    })
}
