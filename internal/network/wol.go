package network

import (
	"fmt"
	"net"
)

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

func SendWakeOnLAN(mac net.HardwareAddr) error {
	// https://superuser.com/questions/295325/does-it-matter-what-udp-port-a-wol-signal-is-sent-to
	// UDP is recommended because it can be generated without raw sockets which come with security restrictions,
	// and port 9 is recommended because it maps to the old well-known discard protocol
	const wolPort = 9
	magicPacket := buildMacicPacket(mac)

	// TODO no IPv6 for now
	conn, err := net.DialUDP("udp4", nil, &net.UDPAddr{
		IP:   net.IPv4bcast,
		Port: wolPort,
	})
	if err != nil {
		return fmt.Errorf("failed to dial UDP broadcast: %w", err)
	}
	defer conn.Close()

	if _, err = conn.Write(magicPacket); err != nil {
		return fmt.Errorf("failed to send magic packet: %w", err)
	}

	return nil
}
