package hosts

import (
	"encoding/json"
	"fmt"
	"net"
	"os"
	"sync"
)

type State int

const (
	// TODO maybe catch pixiecore events and/or ping to add booting/booted state
	StateUnknown State = iota
	StateInstalling
	StateInstalled
	StateFailed
)

type Host struct {
	MACAddress net.HardwareAddr
	IP         string
	state      State
	mu         sync.RWMutex
}

type HostsConfig map[string]*Host

func (h *Host) GetState() State {
	h.mu.RLock()
	defer h.mu.RUnlock()
	return h.state
}

func (h *Host) SetState(state State) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.state = state
}

func LoadHostsConfig(filename string) (HostsConfig, error) {
	data, err := os.ReadFile(filename)
	if err != nil {
		return nil, fmt.Errorf("failed to read hosts file: %w", err)
	}

	var inventory map[string]struct {
		MACAddress string `json:"mac_address"`
		IP         string `json:"ip"`
	}
	if err := json.Unmarshal(data, &inventory); err != nil {
		return nil, fmt.Errorf("failed to parse hosts file: %w", err)
	}
	if inventory == nil {
		return nil, fmt.Errorf("hosts file must contain a JSON object")
	}

	hostsConfig := make(HostsConfig, len(inventory))
	macs := make(map[string]string, len(inventory))
	ips := make(map[string]string, len(inventory))
	for name, entry := range inventory {
		mac, err := net.ParseMAC(entry.MACAddress)
		if err != nil || len(mac) != 6 || mac[0]&1 != 0 || mac.String() == "00:00:00:00:00:00" {
			return nil, fmt.Errorf("host %q has invalid MAC address %q", name, entry.MACAddress)
		}
		ip := net.ParseIP(entry.IP)
		if ip == nil {
			return nil, fmt.Errorf("host %q has invalid IP address %q", name, entry.IP)
		}
		if other, exists := macs[mac.String()]; exists {
			return nil, fmt.Errorf("hosts %q and %q share MAC address %s", other, name, mac)
		}
		if other, exists := ips[ip.String()]; exists {
			return nil, fmt.Errorf("hosts %q and %q share IP address %s", other, name, ip)
		}
		macs[mac.String()] = name
		ips[ip.String()] = name
		hostsConfig[name] = &Host{MACAddress: mac, IP: ip.String()}
	}
	return hostsConfig, nil
}

func GetFlakeOutputByMAC(macAddress string, hostsConfig HostsConfig) (string, error) {
	for flake, config := range hostsConfig {
		if config.MACAddress.String() == macAddress {
			return flake, nil
		}
	}
	return "", fmt.Errorf("unknown MAC address: %s", macAddress)
}

func AllInstalled(hostsConfig HostsConfig) bool {
	for _, host := range hostsConfig {
		if host.GetState() != StateInstalled {
			return false
		}
	}

	return true
}
