//! Wake-on-LAN magic packet construction.

use nixie::network::build_magic_packet;

#[test]
fn builds_magic_packet() {
    let mac = [0xBC, 0x24, 0x11, 0xd0, 0x28, 0x34];
    let packet = build_magic_packet(&mac);

    let mut want = vec![0xFF; 6];
    for _ in 0..16 {
        want.extend_from_slice(&mac);
    }
    assert_eq!(packet, want);
    assert_eq!(packet.len(), 102);
}
