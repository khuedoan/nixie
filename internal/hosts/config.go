package hosts

import (
	"encoding/json"
	"fmt"
	"os"
)

type Host struct {
	MACAddress string `json:"mac_address"`
}

type HostsConfig map[string]Host

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
