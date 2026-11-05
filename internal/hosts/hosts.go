package hosts

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"sync"
)

type State int

const (
	StateUnknown State = iota
	StateInstalling
	StateInstalled
	StateFailed
)

type Host struct {
	MACAddress string       `json:"mac_address"`
	IP         string       `json:"ip"`
	State      State        `json:"-"`
	mu         sync.RWMutex `json:"-"`
}

type HostsConfig map[string]*Host

func (h *Host) GetState() State {
	h.mu.RLock()
	defer h.mu.RUnlock()
	return h.State
}

func (h *Host) SetState(state State) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.State = state
}

func (h *Host) CompareAndSwapState(from, to State) bool {
	h.mu.Lock()
	defer h.mu.Unlock()

	if h.State != from {
		return false
	}

	h.State = to
	return true
}

func LoadHostsConfig(filename string) (HostsConfig, error) {
	data, err := os.ReadFile(filename)
	if err != nil {
		return nil, fmt.Errorf("failed to read hosts file: %w", err)
	}

	var hostsConfig HostsConfig
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()

	if err := decoder.Decode(&hostsConfig); err != nil {
		return nil, fmt.Errorf("failed to parse hosts file: %w", err)
	}

	if decoder.Decode(new(any)) != io.EOF {
		return nil, fmt.Errorf("hosts file must contain one JSON object")
	}

	macOwners := make(map[string]string, len(hostsConfig))
	ipOwners := make(map[string]string, len(hostsConfig))

	for name, host := range hostsConfig {
		if host == nil {
			return nil, fmt.Errorf("host %q must be an object", name)
		}

		mac, err := net.ParseMAC(host.MACAddress)
		if err != nil {
			return nil, fmt.Errorf("host %q has invalid MAC address %q: %w", name, host.MACAddress, err)
		}

		host.MACAddress = mac.String()
		if owner, exists := macOwners[host.MACAddress]; exists {
			return nil, fmt.Errorf("hosts %q and %q share MAC address %q", owner, name, host.MACAddress)
		}

		macOwners[host.MACAddress] = name

		ip := net.ParseIP(host.IP)
		if ip == nil {
			return nil, fmt.Errorf("host %q has invalid IP address %q", name, host.IP)
		}

		host.IP = ip.String()
		if owner, exists := ipOwners[host.IP]; exists {
			return nil, fmt.Errorf("hosts %q and %q share IP address %q", owner, name, host.IP)
		}

		ipOwners[host.IP] = name
	}

	return hostsConfig, nil
}

func GetFlakeOutputByMAC(macAddress string, hostsConfig HostsConfig) (string, error) {
	for flake, config := range hostsConfig {
		if config.MACAddress == macAddress {
			return flake, nil
		}
	}

	return "", fmt.Errorf("unknown MAC address: %s", macAddress)
}
