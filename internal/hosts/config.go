package hosts

import (
	"encoding/json"
	"fmt"
	"net"
	"os"
)

type Host struct {
	MACAddress net.HardwareAddr `json:"mac_address"`
}

type HostsConfig map[string]Host

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
	return nil
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
