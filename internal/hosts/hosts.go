package hosts

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"strings"
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
	MACAddress    net.HardwareAddr `json:"mac_address"`
	IP            string           `json:"ip,omitempty"`
	MachineIDHash string           `json:"machine_id_hash,omitempty"`
	State         State            `json:"-"`
	mu            sync.RWMutex     `json:"-"`
}

type HostsConfig map[string]*Host

func (h *Host) UnmarshalJSON(data []byte) error {
	var aux struct {
		MACAddress    string `json:"mac_address"`
		IP            string `json:"ip"`
		MachineIDHash string `json:"machine_id_hash"`
	}

	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}

	mac, err := net.ParseMAC(aux.MACAddress)
	if err != nil {
		return fmt.Errorf("invalid MAC address %q: %w", aux.MACAddress, err)
	}

	h.MACAddress = mac
	if aux.IP != "" {
		ip := net.ParseIP(aux.IP)
		if ip == nil {
			return fmt.Errorf("invalid IP address %q", aux.IP)
		}
		h.IP = ip.String()
	}
	h.MachineIDHash = strings.TrimSpace(aux.MachineIDHash)
	return nil
}

func (h *Host) MarshalJSON() ([]byte, error) {
	h.mu.RLock()
	defer h.mu.RUnlock()

	return json.Marshal(struct {
		MACAddress    string `json:"mac_address"`
		IP            string `json:"ip,omitempty"`
		MachineIDHash string `json:"machine_id_hash,omitempty"`
	}{
		MACAddress:    h.MACAddress.String(),
		IP:            h.IP,
		MachineIDHash: h.MachineIDHash,
	})
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

func (h *Host) SetFinalIdentity(ip string, machineIDHash string) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.IP = ip
	h.MachineIDHash = machineIDHash
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

func SaveHostsConfig(filename string, hostsConfig HostsConfig) error {
	data, err := json.MarshalIndent(hostsConfig, "", "  ")
	if err != nil {
		return fmt.Errorf("failed to encode hosts file: %w", err)
	}
	data = append(data, '\n')

	if err := os.WriteFile(filename, data, 0o644); err != nil {
		return fmt.Errorf("failed to write hosts file: %w", err)
	}

	return nil
}

func HashMachineID(machineID string) (string, error) {
	machineID = strings.ToLower(strings.TrimSpace(machineID))
	if len(machineID) != 32 {
		return "", fmt.Errorf("invalid machine ID length: got %d, want 32", len(machineID))
	}
	if _, err := hex.DecodeString(machineID); err != nil {
		return "", fmt.Errorf("invalid machine ID %q: %w", machineID, err)
	}

	// systemd treats /etc/machine-id as confidential. Store an app-specific hash
	// instead of the raw ID so hosts can be identified without exposing it.
	// https://www.freedesktop.org/software/systemd/man/latest/machine-id.html
	mac := hmac.New(sha256.New, []byte("code.khuedoan.com/nixie/machine-id/v1"))
	mac.Write([]byte(machineID))
	return hex.EncodeToString(mac.Sum(nil)), nil
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
