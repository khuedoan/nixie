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
	MACAddress net.HardwareAddr `json:"mac_address"`
	State      State            `json:"-"`
	mu         sync.RWMutex     `json:"-"`
}

type HostsConfig map[string]*Host

func (h *Host) UnmarshalJSON(data []byte) error {
	var aux struct {
		MACAddress string `json:"mac_address"`
	}

	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}

	mac, err := net.ParseMAC(aux.MACAddress)
	if err != nil {
		return fmt.Errorf("invalid MAC address %q: %w", aux.MACAddress, err)
	}

	h.MACAddress = mac
	h.State = StateUnknown
	return nil
}

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

func LoadHostsConfig(filename string) (HostsConfig, error) {
	data, err := os.ReadFile(filename)
	if err != nil {
		return nil, fmt.Errorf("failed to read hosts file: %w", err)
	}

	var hostsConfig HostsConfig
	if err := json.Unmarshal(data, &hostsConfig); err != nil {
		return nil, fmt.Errorf("failed to parse hosts file: %w", err)
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
