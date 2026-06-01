package hosts

import (
	"os"
	"strings"
	"testing"
)

func TestHostsConfigRoundTrip(t *testing.T) {
	path := t.TempDir() + "/hosts.json"
	original := `{
  "machine1": {
    "mac_address": "bc:24:11:d0:28:34",
    "ip": "192.168.1.42",
    "machine_id_hash": "abc123"
  }
}`
	if err := os.WriteFile(path, []byte(original), 0o644); err != nil {
		t.Fatal(err)
	}

	hostsConfig, err := LoadHostsConfig(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := SaveHostsConfig(path, hostsConfig); err != nil {
		t.Fatal(err)
	}
	if data, err := os.ReadFile(path); err != nil {
		t.Fatal(err)
	} else if strings.Contains(string(data), "State") {
		t.Fatalf("saved hosts file contains transient state: %s", data)
	}

	hostsConfig, err = LoadHostsConfig(path)
	if err != nil {
		t.Fatal(err)
	}
	host := hostsConfig["machine1"]
	if host == nil ||
		host.MACAddress.String() != "bc:24:11:d0:28:34" ||
		host.IP != "192.168.1.42" ||
		host.MachineIDHash != "abc123" ||
		host.GetState() != StateUnknown {
		t.Fatalf("unexpected host after round trip: %+v", host)
	}
}

func TestHashMachineID(t *testing.T) {
	machineID := "0123456789abcdef0123456789abcdef"
	machineIDHash, err := HashMachineID(machineID + "\n")
	if err != nil {
		t.Fatal(err)
	}
	if len(machineIDHash) != 64 || machineIDHash != strings.ToLower(machineIDHash) || machineIDHash == machineID {
		t.Fatalf("invalid hash for machine ID: %q", machineIDHash)
	}
	if _, err := HashMachineID("not-a-machine-id"); err == nil {
		t.Fatal("HashMachineID accepted invalid machine ID")
	}
}
