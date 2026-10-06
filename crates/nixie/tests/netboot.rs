//! Netboot unit tests: real DISCOVER decoding, iPXE script, TFTP transfer.

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dhcproto::v4::{DhcpOption, MessageType, Opcode, OptionCode};
use nixie::mac::MacAddr;
use nixie::netboot::booter::{Architecture, BootSpec, Machine};
use nixie::netboot::dhcp::{self, BootError, Firmware, PxeOptions, OPTION_CLIENT_GUID};
use nixie::netboot::http::{expand_cmdline, ipxe_script, query_param};
use nixie::netboot::tftp::TftpServer;

/// Real PXE DHCPDISCOVER from `internal/netboot/dhcp4/testdata/dhcp.pcap`.
const DISCOVER: &[u8] = include_bytes!("fixtures/discover_bios.bin");
const MAC: &str = "d0:50:99:4e:05:57";
const GUID: [u8; 17] = [
    0x00, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04, 0x00, 0x05, 0x00, 0x06, 0x00, 0x07, 0x00, 0x08, 0x00,
    0x09,
];

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

fn with_arch(raw: &[u8], arch: u16) -> Vec<u8> {
    let mut out = raw.to_vec();
    let i = find_option(raw, 93).expect("option 93 present");
    out[i + 2..i + 4].copy_from_slice(&arch.to_be_bytes());
    out
}

#[test]
fn decodes_real_discover_and_extracts_pxe_options() {
    let msg = dhcp::decode(DISCOVER).expect("real DISCOVER decodes");
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
fn classifies_real_bios_discover_and_builds_offer() {
    let msg = dhcp::decode(DISCOVER).unwrap();
    dhcp::is_boot_dhcp(&msg).expect("is a boot DISCOVER");

    let class = dhcp::validate_dhcp(&msg).expect("valid");
    assert_eq!(class.machine.mac.to_string(), MAC);
    assert_eq!(class.machine.arch, Architecture::IA32);
    assert_eq!(class.firmware, Firmware::X86PC);

    let server_ip = "192.168.16.1".parse().unwrap();
    let offer = dhcp::offer_dhcp(&msg, &class, server_ip, 80);

    assert_eq!(offer.opcode(), Opcode::BootReply);
    assert_eq!(offer.xid(), msg.xid());
    assert!(offer.flags().broadcast());
    assert_eq!(offer.chaddr(), [0xd0, 0x50, 0x99, 0x4e, 0x05, 0x57]);
    assert_eq!(offer.siaddr(), server_ip);
    assert_eq!(
        offer.sname_str().unwrap().unwrap().trim_end_matches('\0'),
        "192.168.16.1"
    );
    assert_eq!(
        offer.fname_str().unwrap().unwrap().trim_end_matches('\0'),
        "d0:50:99:4e:05:57/0"
    );
    assert_eq!(
        offer.opts().get(OptionCode::ServerIdentifier),
        Some(&DhcpOption::ServerIdentifier(server_ip))
    );
    assert_eq!(
        offer.opts().get(OptionCode::ClassIdentifier),
        Some(&DhcpOption::ClassIdentifier(b"PXEClient".to_vec()))
    );
    assert_eq!(
        offer.opts().get(OptionCode::VendorExtensions),
        Some(&DhcpOption::VendorExtensions(vec![6, 1, 8, 255]))
    );
    assert_eq!(
        offer.opts().get(OPTION_CLIENT_GUID),
        Some(&DhcpOption::ClientMachineIdentifier(GUID.to_vec()))
    );
    assert_eq!(offer.opts().msg_type(), Some(MessageType::Offer));
}

#[test]
fn builds_efi_offers_without_option_43() {
    for (arch, firmware) in [
        (6u16, Firmware::EFI32),
        (7, Firmware::EFI64),
        (9, Firmware::EFIBC),
    ] {
        let raw = with_arch(DISCOVER, arch);
        let msg = dhcp::decode(&raw).unwrap();
        let class = dhcp::validate_dhcp(&msg).unwrap();
        assert_eq!(class.firmware, firmware);

        let server_ip = "10.0.0.1".parse().unwrap();
        let offer = dhcp::offer_dhcp(&msg, &class, server_ip, 80);
        assert_eq!(offer.siaddr(), server_ip);
        assert_eq!(offer.opts().get(OptionCode::VendorExtensions), None);
        assert_eq!(
            offer.opts().get(OptionCode::ClassIdentifier),
            Some(&DhcpOption::ClassIdentifier(b"PXEClient".to_vec()))
        );
        assert_eq!(
            offer.fname_str().unwrap().unwrap().trim_end_matches('\0'),
            format!("{MAC}/{}", firmware.number())
        );
    }
}

#[test]
fn rejects_malformed_guid_and_non_discover() {
    let i = find_option(DISCOVER, 53).unwrap();
    let mut raw = DISCOVER.to_vec();
    raw[i + 2] = 2;
    assert_eq!(
        dhcp::is_boot_dhcp(&dhcp::decode(&raw).unwrap()),
        Err(BootError::NotDiscover)
    );

    let i = find_option(DISCOVER, 97).unwrap();
    let mut raw = DISCOVER.to_vec();
    raw[i + 2] = 1;
    assert_eq!(
        dhcp::validate_dhcp(&dhcp::decode(&raw).unwrap()),
        Err(BootError::BadGuidLeadByte(1))
    );

    let msg = dhcp::decode(&with_arch(DISCOVER, 0xdead)).unwrap();
    assert_eq!(
        dhcp::validate_dhcp(&msg),
        Err(BootError::UnsupportedClientArch(0xdead))
    );
}

#[test]
fn builds_ipxe_script_matching_pixiecore() {
    let machine = Machine {
        mac: "01:02:03:04:05:06".parse::<MacAddr>().unwrap(),
        arch: Architecture::IA32,
    };
    let spec = BootSpec {
        kernel: "k-01:02:03:04:05:06-0".to_string(),
        initrd: vec![
            "i1-01:02:03:04:05:06-0".to_string(),
            "i2-01:02:03:04:05:06-0".to_string(),
        ],
        cmdline: "thing={{ ID \"f-01:02:03:04:05:06-0\" }} foo=bar".to_string(),
        ..BootSpec::default()
    };

    let script = ipxe_script(machine, &spec, "localhost:1234").unwrap();
    let expected = "\
#!ipxe
kernel --name kernel http://localhost:1234/_/file?name=k-01%3A02%3A03%3A04%3A05%3A06-0&type=kernel&mac=01%3A02%3A03%3A04%3A05%3A06
initrd --name initrd0 http://localhost:1234/_/file?name=i1-01%3A02%3A03%3A04%3A05%3A06-0&type=initrd&mac=01%3A02%3A03%3A04%3A05%3A06
initrd --name initrd1 http://localhost:1234/_/file?name=i2-01%3A02%3A03%3A04%3A05%3A06-0&type=initrd&mac=01%3A02%3A03%3A04%3A05%3A06
imgfetch --name ready http://localhost:1234/_/booting?mac=01%3A02%3A03%3A04%3A05%3A06 ||
imgfree ready ||
boot kernel initrd=initrd0 initrd=initrd1 thing=http://localhost:1234/_/file?name=f-01%3A02%3A03%3A04%3A05%3A06-0 foo=bar
";
    assert_eq!(String::from_utf8(script).unwrap(), expected);
}

#[test]
fn expands_cmdline_and_parses_query_params() {
    assert_eq!(
        expand_cmdline("a={{ ID \"x\" }} b", |id| format!("/f?name={id}")).unwrap(),
        "a=/f?name=x b"
    );
    assert!(expand_cmdline("a\nb", |_| String::new()).is_err());
    assert_eq!(
        query_param("/_/ipxe?mac=01%3A02%3A03%3A04%3A05%3A06&arch=1", "mac").as_deref(),
        Some("01:02:03:04:05:06")
    );
    assert_eq!(query_param("/_/ipxe?arch=1", "mac"), None);
}

#[test]
fn serves_a_file_over_tftp() {
    let payload: &[u8] = b"ipxe firmware bytes";
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let shutdown = Arc::new(AtomicBool::new(false));
    let handler = Arc::new(move |_path: &str| Ok((payload.to_vec(), payload.len() as u64)));
    let server = TftpServer::new(handler);
    let server_shutdown = Arc::clone(&shutdown);
    let handle = std::thread::spawn(move || server.serve(socket, server_shutdown));

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut rrq = vec![0, 1];
    rrq.extend_from_slice(b"d0:50:99:4e:05:57/2\0octet\0");
    client.send_to(&rrq, ("127.0.0.1", port)).unwrap();

    let mut buf = [0u8; 2048];
    let (n, from) = client.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..2], &[0, 3], "data packet");
    assert_eq!(&buf[2..4], &[0, 1], "first block");
    assert_eq!(&buf[4..n], payload);

    // Acknowledge so the server can finish.
    client.send_to(&[0, 4, 0, 1], from).unwrap();

    shutdown.store(true, Ordering::Relaxed);
    let _ = handle.join();
}

#[test]
fn serves_a_file_over_tftp_with_block_size_option() {
    let payload: &[u8] = b"0123456789";
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let shutdown = Arc::new(AtomicBool::new(false));
    let handler = Arc::new(move |_path: &str| Ok((payload.to_vec(), payload.len() as u64)));
    let server = TftpServer::new(handler);
    let server_shutdown = Arc::clone(&shutdown);
    let handle = std::thread::spawn(move || server.serve(socket, server_shutdown));

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut rrq = vec![0, 1];
    rrq.extend_from_slice(b"file\x00octet\x00blksize\x001024\x00tsize\x000\x00");
    client.send_to(&rrq, ("127.0.0.1", port)).unwrap();

    let mut buf = [0u8; 2048];
    let (n, from) = client.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..2], &[0, 6], "OACK");
    let oack = String::from_utf8_lossy(&buf[2..n]);
    assert!(oack.contains("blksize\x001024"), "OACK: {oack:?}");
    assert!(oack.contains("tsize\x0010"), "OACK: {oack:?}");
    client.send_to(&[0, 4, 0, 0], from).unwrap();

    let (n, _) = client.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..2], &[0, 3]);
    assert_eq!(&buf[4..n], payload);

    shutdown.store(true, Ordering::Relaxed);
    let _ = handle.join();
}
