//! Decode a real captured PXE DHCPDISCOVER and assert the classification and
//! the ProxyDHCP offer fields. These tests do not need any privileges.

use std::net::Ipv4Addr;

use dhcproto::v4::{DhcpOption, MessageType, Opcode, OptionCode};
use proxydhcp_snooper::proxy::{
    decode, encode, handle_datagram, is_boot_dhcp, offer_dhcp, validate_dhcp, BootError,
    OfferConfig, PxeOptions,
};
use proxydhcp_snooper::{Architecture, Firmware};

/// A real PXE DHCPDISCOVER captured in `internal/netboot/dhcp4/testdata/dhcp.pcap`
/// (the UDP payload of packet 1). MAC d0:50:99:4e:05:57, arch 0 (x86 BIOS),
/// vendor class "PXEClient:Arch:00000:UNDI:002001", 17-byte GUID option 97.
const DISCOVER: &[u8] = include_bytes!("fixtures/discover_bios.bin");

const MAC: &str = "d0:50:99:4e:05:57";
const GUID: [u8; 17] = [
    0x00, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04, 0x00, 0x05, 0x00, 0x06, 0x00, 0x07, 0x00, 0x08, 0x00,
    0x09,
];

// --- helpers for editing the captured packet -------------------------------------------------

/// Return the offset of the option code byte for `code`, scanning the options
/// section that starts after the 236-byte BOOTP header and 4-byte magic.
fn find_option(raw: &[u8], code: u8) -> Option<usize> {
    let mut i = 240;
    while i + 1 < raw.len() {
        match raw[i] {
            0 => i += 1,
            255 => return None,
            c => {
                let len = raw[i + 1] as usize;
                if c == code {
                    return Some(i);
                }
                i += 2 + len;
            }
        }
    }
    None
}

/// Overwrite the two-byte option 93 value, keeping the packet otherwise identical.
fn with_arch(raw: &[u8], arch: u16) -> Vec<u8> {
    let mut out = raw.to_vec();
    let i = find_option(raw, 93).expect("option 93 present");
    assert_eq!(out[i + 1], 2, "option 93 is two bytes");
    out[i + 2..i + 4].copy_from_slice(&arch.to_be_bytes());
    out
}

/// Splice a new option in just before the End marker.
fn with_option(raw: &[u8], code: u8, value: &[u8]) -> Vec<u8> {
    let mut end = 240;
    while end < raw.len() && raw[end] != 255 {
        end += 2 + raw[end + 1] as usize;
    }
    assert_eq!(raw[end], 255, "options are terminated");
    let mut out = raw[..end].to_vec();
    out.push(code);
    out.push(value.len() as u8);
    out.extend_from_slice(value);
    out.push(255);
    out.extend_from_slice(&raw[end + 1..]);
    out
}

/// Remove `drop` bytes from the end of an option's value and shrink its length byte.
fn shrink_option(raw: &[u8], code: u8, drop: usize) -> Vec<u8> {
    let i = find_option(raw, code).expect("option present");
    let len = raw[i + 1] as usize;
    let mut out = raw.to_vec();
    out[i + 1] = (len - drop) as u8;
    out.drain(i + 2 + (len - drop)..i + 2 + len);
    out
}

// --- tests -----------------------------------------------------------------------------------

/// `dhcproto` includes the NUL terminator when decoding the fixed-size `sname`
/// and `file` fields, so trim it before comparing.
fn text(value: Option<Result<&str, std::str::Utf8Error>>) -> String {
    value
        .expect("field present")
        .expect("field is UTF-8")
        .trim_end_matches('\0')
        .to_string()
}

#[test]
fn decodes_real_discover_and_extracts_pxe_options() {
    let msg = decode(DISCOVER).expect("real DISCOVER decodes");
    assert_eq!(msg.opts().msg_type(), Some(MessageType::Discover));
    assert_eq!(msg.xid(), 0x9b4e_0557);
    assert_eq!(msg.chaddr(), [0xd0, 0x50, 0x99, 0x4e, 0x05, 0x57]);

    let pxe = PxeOptions::from_message(&msg);
    assert_eq!(pxe.client_arch, Some(0));
    assert_eq!(
        pxe.vendor_class.as_deref(),
        Some(&b"PXEClient:Arch:00000:UNDI:002001"[..])
    );
    assert_eq!(pxe.user_class, None);
    assert_eq!(pxe.client_guid.as_deref(), Some(&GUID[..]));
}

#[test]
fn classifies_real_bios_discover() {
    let msg = decode(DISCOVER).unwrap();
    is_boot_dhcp(&msg).expect("is a boot DISCOVER");

    let class = validate_dhcp(&msg).expect("valid");
    assert_eq!(class.machine.mac.to_string(), MAC);
    assert_eq!(class.machine.arch, Architecture::IA32);
    assert_eq!(class.firmware, Firmware::X86PC);
}

#[test]
fn builds_bios_offer() {
    let msg = decode(DISCOVER).unwrap();
    let class = validate_dhcp(&msg).unwrap();
    let server_ip: Ipv4Addr = "192.168.16.1".parse().unwrap();
    let offer = offer_dhcp(&msg, &class, &OfferConfig::new(server_ip, 80));

    assert_eq!(offer.opcode(), Opcode::BootReply);
    assert_eq!(offer.xid(), msg.xid());
    assert!(offer.flags().broadcast());
    assert_eq!(offer.chaddr(), [0xd0, 0x50, 0x99, 0x4e, 0x05, 0x57]);
    assert_eq!(offer.giaddr(), msg.giaddr());
    assert_eq!(offer.siaddr(), server_ip);
    assert_eq!(offer.sname_str().unwrap().unwrap(), "192.168.16.1");
    assert_eq!(offer.fname_str().unwrap().unwrap(), "d0:50:99:4e:05:57/0");

    assert_eq!(
        offer.opts().get(OptionCode::ServerIdentifier),
        Some(&DhcpOption::ServerIdentifier(server_ip))
    );
    assert_eq!(
        offer.opts().get(OptionCode::ClassIdentifier),
        Some(&DhcpOption::ClassIdentifier(b"PXEClient".to_vec()))
    );
    // Option 43: PXE boot server discovery control, bypass, plus End marker.
    assert_eq!(
        offer.opts().get(OptionCode::VendorExtensions),
        Some(&DhcpOption::VendorExtensions(vec![6, 1, 8, 255]))
    );
    assert_eq!(
        offer.opts().get(OptionCode::ClientMachineIdentifier),
        Some(&DhcpOption::ClientMachineIdentifier(GUID.to_vec()))
    );
    assert_eq!(offer.opts().msg_type(), Some(MessageType::Offer));

    // The offer must survive a wire round trip.
    let raw = encode(&offer).unwrap();
    let back = decode(&raw).unwrap();
    assert_eq!(back.opcode(), Opcode::BootReply);
    assert_eq!(text(back.fname_str()), "d0:50:99:4e:05:57/0");
    assert_eq!(text(back.sname_str()), "192.168.16.1");
    assert_eq!(back.opts().msg_type(), Some(MessageType::Offer));
}

#[test]
fn builds_efi_offers_without_option_43() {
    for (arch, expected_fw, expected_arch) in [
        (6u16, Firmware::EFI32, Architecture::IA32),
        (7, Firmware::EFI64, Architecture::X64),
        (9, Firmware::EFIBC, Architecture::X64),
    ] {
        let raw = with_arch(DISCOVER, arch);
        let msg = decode(&raw).unwrap();
        let class = validate_dhcp(&msg).expect("valid EFI request");
        assert_eq!(class.firmware, expected_fw);
        assert_eq!(class.machine.arch, expected_arch);

        let server_ip: Ipv4Addr = "10.0.0.1".parse().unwrap();
        let offer = offer_dhcp(&msg, &class, &OfferConfig::new(server_ip, 80));

        assert_eq!(offer.siaddr(), server_ip);
        assert_eq!(
            offer.opts().get(OptionCode::ServerIdentifier),
            Some(&DhcpOption::ServerIdentifier(server_ip))
        );
        assert_eq!(
            offer.opts().get(OptionCode::ClassIdentifier),
            Some(&DhcpOption::ClassIdentifier(b"PXEClient".to_vec()))
        );
        assert_eq!(
            offer.opts().get(OptionCode::ClientMachineIdentifier),
            Some(&DhcpOption::ClientMachineIdentifier(GUID.to_vec()))
        );
        // EFI intentionally omits option 43 and sets sname + filename.
        assert_eq!(offer.opts().get(OptionCode::VendorExtensions), None);
        assert_eq!(offer.sname_str().unwrap().unwrap(), "10.0.0.1");
        assert_eq!(
            offer.fname_str().unwrap().unwrap(),
            format!("{MAC}/{}", expected_fw.number())
        );
    }
}

#[test]
fn user_class_selects_ipxe_firmware_variants() {
    let raw = with_option(DISCOVER, 77, b"iPXE");
    let msg = decode(&raw).unwrap();
    let class = validate_dhcp(&msg).unwrap();
    assert_eq!(class.machine.arch, Architecture::IA32);
    assert_eq!(class.firmware, Firmware::X86Ipxe);

    let server_ip: Ipv4Addr = "10.0.0.1".parse().unwrap();
    let offer = offer_dhcp(&msg, &class, &OfferConfig::new(server_ip, 80));
    assert_eq!(
        offer.fname_str().unwrap().unwrap(),
        "tftp://10.0.0.1/d0:50:99:4e:05:57/4"
    );

    let raw = with_option(DISCOVER, 77, b"pixiecore");
    let msg = decode(&raw).unwrap();
    let class = validate_dhcp(&msg).unwrap();
    assert_eq!(class.firmware, Firmware::PixiecoreIpxe);

    let offer = offer_dhcp(&msg, &class, &OfferConfig::new(server_ip, 8080));
    assert_eq!(
        offer.fname_str().unwrap().unwrap(),
        "http://10.0.0.1:8080/_/ipxe?arch=0&mac=d0:50:99:4e:05:57"
    );
}

#[test]
fn handle_datagram_runs_the_whole_policy() {
    let cfg = OfferConfig::new("192.168.16.1".parse().unwrap(), 80);
    let resp = handle_datagram(DISCOVER, &cfg).expect("offer built");

    assert_eq!(resp.classification.machine.mac.to_string(), MAC);
    assert_eq!(resp.classification.firmware, Firmware::X86PC);

    let offer = decode(&resp.bytes).unwrap();
    assert_eq!(offer.opts().msg_type(), Some(MessageType::Offer));
    assert_eq!(text(offer.fname_str()), "d0:50:99:4e:05:57/0");
}

#[test]
fn rejects_non_discover() {
    // Flip the DHCP message type option (53) from Discover (1) to Offer (2).
    let i = find_option(DISCOVER, 53).unwrap();
    let mut raw = DISCOVER.to_vec();
    raw[i + 2] = 2;
    let msg = decode(&raw).unwrap();
    assert_eq!(is_boot_dhcp(&msg), Err(BootError::NotDiscover));
}

#[test]
fn rejects_malformed_guid() {
    let i = find_option(DISCOVER, 97).unwrap();
    let mut raw = DISCOVER.to_vec();
    raw[i + 2] = 1; // leading byte must be zero
    let msg = decode(&raw).unwrap();
    assert_eq!(validate_dhcp(&msg), Err(BootError::BadGuidLeadByte(1)));

    let raw = shrink_option(DISCOVER, 97, 1);
    let msg = decode(&raw).unwrap();
    assert_eq!(validate_dhcp(&msg), Err(BootError::BadGuidLength(16)));
}

#[test]
fn rejects_unsupported_architecture() {
    let msg = decode(&with_arch(DISCOVER, 0xdead)).unwrap();
    assert_eq!(
        validate_dhcp(&msg),
        Err(BootError::UnsupportedClientArch(0xdead))
    );
}
