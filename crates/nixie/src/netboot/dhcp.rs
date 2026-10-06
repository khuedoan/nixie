//! ProxyDHCP classification and offer construction.
//!
//! Mirrors Nixie's Go pixiecore fork: `isBootDHCP`, `validateDHCP` and
//! `offerDHCP` in `internal/netboot/pixiecore/dhcp.go`. The DHCPv4 wire format
//! is handled by `dhcproto`; everything here is a pure function of decoded
//! messages so it can be tested without privileges.

use std::error::Error;
use std::fmt;
use std::net::Ipv4Addr;

use dhcproto::error::{DecodeError, EncodeError};
use dhcproto::v4::{DhcpOption, Flags, Message, MessageType, Opcode, OptionCode};
use dhcproto::{Decodable, Decoder, Encodable, Encoder};

use super::booter::{Architecture, Machine};
use crate::mac::MacAddr;

/// Option 93: client system architecture (PXE).
pub const OPTION_CLIENT_ARCH: OptionCode = OptionCode::ClientSystemArchitecture;
/// Option 97: client machine identifier (GUID). PXE requires a leading zero byte.
pub const OPTION_CLIENT_GUID: OptionCode = OptionCode::ClientMachineIdentifier;
/// Option 60: vendor class identifier ("PXEClient:Arch:...").
pub const OPTION_VENDOR_CLASS: OptionCode = OptionCode::ClassIdentifier;
/// Option 77: user class ("iPXE", "pixiecore", ...).
pub const OPTION_USER_CLASS: OptionCode = OptionCode::UserClass;

/// The PXE-relevant options extracted from a decoded DISCOVER.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PxeOptions {
    pub client_arch: Option<u16>,
    pub client_guid: Option<Vec<u8>>,
    pub vendor_class: Option<Vec<u8>>,
    pub user_class: Option<Vec<u8>>,
}

impl PxeOptions {
    pub fn from_message(msg: &Message) -> Self {
        let opts = msg.opts();
        let client_arch = match opts.get(OPTION_CLIENT_ARCH) {
            Some(DhcpOption::ClientSystemArchitecture(arch)) => Some(u16::from(*arch)),
            _ => None,
        };
        Self {
            client_arch,
            client_guid: match opts.get(OPTION_CLIENT_GUID) {
                Some(DhcpOption::ClientMachineIdentifier(v)) => Some(v.clone()),
                _ => None,
            },
            vendor_class: match opts.get(OPTION_VENDOR_CLASS) {
                Some(DhcpOption::ClassIdentifier(v)) => Some(v.clone()),
                _ => None,
            },
            user_class: match opts.get(OPTION_USER_CLASS) {
                Some(DhcpOption::UserClass(v)) => Some(v.clone()),
                _ => None,
            },
        }
    }
}

/// Firmware type. The discriminants match Go's `Firmware` iota because the
/// numeric value is embedded in the boot filename.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Firmware {
    X86PC = 0,
    EFI32 = 1,
    EFI64 = 2,
    EFIBC = 3,
    X86Ipxe = 4,
    PixiecoreIpxe = 5,
}

impl Firmware {
    pub fn number(self) -> u8 {
        self as u8
    }
}

/// The result of validating a boot request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    pub machine: Machine,
    pub firmware: Firmware,
}

/// Why a packet is not a usable PXE boot request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootError {
    NotDiscover,
    MissingMessageType,
    MissingClientArch,
    UnsupportedClientArch(u16),
    BadHardwareAddrLength(u8),
    BadGuidLength(usize),
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

#[derive(Debug)]
pub enum DhcpError {
    Decode(DecodeError),
    Boot(BootError),
    Encode(EncodeError),
}

impl fmt::Display for DhcpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DhcpError::Decode(e) => write!(f, "decoding DHCP packet: {e}"),
            DhcpError::Boot(e) => write!(f, "{e}"),
            DhcpError::Encode(e) => write!(f, "encoding DHCP offer: {e}"),
        }
    }
}

impl Error for DhcpError {}

impl From<DecodeError> for DhcpError {
    fn from(e: DecodeError) -> Self {
        DhcpError::Decode(e)
    }
}
impl From<BootError> for DhcpError {
    fn from(e: BootError) -> Self {
        DhcpError::Boot(e)
    }
}
impl From<EncodeError> for DhcpError {
    fn from(e: EncodeError) -> Self {
        DhcpError::Encode(e)
    }
}

pub fn decode(bytes: &[u8]) -> Result<Message, DecodeError> {
    Message::decode(&mut Decoder::new(bytes))
}

pub fn encode(msg: &Message) -> Result<Vec<u8>, EncodeError> {
    let mut buf = Vec::new();
    let mut enc = Encoder::new(&mut buf);
    msg.encode(&mut enc)?;
    Ok(buf)
}

/// Mirrors Go's `isBootDHCP`: only DISCOVERs carrying option 93 are boot requests.
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

    if let Some(user_class) = pxe.user_class.as_deref() {
        if user_class == b"iPXE" && firmware == Firmware::X86PC {
            firmware = Firmware::X86Ipxe;
        }
        if user_class == b"pixiecore" {
            firmware = Firmware::PixiecoreIpxe;
        }
    }

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
    let mut octets = [0u8; 6];
    octets.copy_from_slice(msg.chaddr());
    let mac = MacAddr::new(octets);

    Ok(Classification {
        machine: Machine { mac, arch },
        firmware,
    })
}

/// Mirrors Go's `offerDHCP`: build the ProxyDHCP offer for a request.
pub fn offer_dhcp(
    request: &Message,
    class: &Classification,
    server_ip: Ipv4Addr,
    http_port: u16,
) -> Message {
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
                "http://{server_ip}:{http_port}/_/ipxe?arch={}&mac={}",
                class.machine.arch.number(),
                class.machine.mac
            ));
        }
    }

    resp
}

/// A complete ProxyDHCP response ready for the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyResponse {
    pub bytes: Vec<u8>,
    pub classification: Classification,
}

/// The full proxy policy without a booter: decode, filter, classify, build an
/// offer. Used by tests and by the server after the booter has accepted.
pub fn handle_datagram(
    raw: &[u8],
    server_ip: Ipv4Addr,
    http_port: u16,
) -> Result<ProxyResponse, DhcpError> {
    let request = decode(raw)?;
    is_boot_dhcp(&request)?;
    let classification = validate_dhcp(&request)?;
    let offer = offer_dhcp(&request, &classification, server_ip, http_port);
    Ok(ProxyResponse {
        bytes: encode(&offer)?,
        classification,
    })
}
