package network

import (
	"fmt"
	"net"

	"github.com/charmbracelet/log"
)

// Adapted from Pixiecore's DHCP logic (Apache License 2.0)
// https://github.com/danderson/netboot/blob/main/pixiecore/dhcp.go#L247-L278
// We need this because the nixie-agent also needs to know the API server address to send its API requests
func DetectServerAddress() (string, error) {
	addresses, err := net.InterfaceAddrs()
	if err != nil {
		return "", fmt.Errorf("failed to get interface addresses: %w", err)
	}

	log.Debug("interface addresses", "addresses", addresses)

	// Try to find an IPv4 address to use
	fs := [](func(net.IP) bool){
		net.IP.IsGlobalUnicast,
		net.IP.IsLinkLocalUnicast,
	}
	for _, f := range fs {
		for _, a := range addresses {
			ipaddr, ok := a.(*net.IPNet)
			if !ok {
				continue
			}
			// TODO support IPv6, probably need to fork pixiecore?
			// Reference fork https://github.com/dmitri-d/netboot/tree/dhcpv6
			ip := ipaddr.IP.To4()
			if ip == nil {
				continue
			}
			if f(ip) {
				return ip.String(), nil
			}
		}
	}

	return "", fmt.Errorf("no usable unicast address")
}
