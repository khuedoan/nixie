package network

import "net"

func buildMacicPacket(mac net.HardwareAddr) []byte {
	// A physical WakeOnLAN (Magic Packet) will look like this:
	//
	// | Synchronization Stream | Target MAC | Password (optional) |
	// | 6                      | 96         | 0, 4 or 6           |
	//
	// See also https://wiki.wireshark.org/WakeOnLAN
	packet := make([]byte, 0)

	const (
		// The synchronization stream is defined as 6 bytes of FFh
		syncStreamLength = 6
		// The Target MAC block contains 16 duplications of the IEEE address of the target
		macRepeat = 16
		// We don't support passwords, at least not yet
	)

	for range syncStreamLength {
		packet = append(packet, 0xFF)
	}
	for range macRepeat {
		packet = append(packet, mac...)
	}

	return packet
}
