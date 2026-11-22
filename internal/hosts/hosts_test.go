package hosts

import (
	"encoding/json"
	"os"
	"strings"
	"testing"
)

func TestInventoryRequiresIP(t *testing.T) {
	path := t.TempDir() + "/hosts.json"

	if err := os.WriteFile(path, []byte(`{"node":{"mac_address":"02:00:00:00:00:01"}}`), 0o600); err != nil {
		t.Fatal(err)
	}

	if _, err := LoadHostsConfig(path); err == nil {
		t.Fatal("accepted a host without its required IP address")
	}
}

func TestInventoryRejectsDuplicateIdentities(t *testing.T) {
	for _, test := range []struct {
		name   string
		field  string
		first  string
		second string
	}{
		{"normalized MAC", "mac_address", "02:00:00:00:00:0a", "02-00-00-00-00-0A"},
		{"IP", "ip", "192.0.2.10", "192.0.2.10"},
		{"normalized IPv6", "ip", "2001:db8::10", "2001:0db8:0:0:0:0:0:0010"},
	} {
		t.Run(test.name, func(t *testing.T) {
			inventory := map[string]map[string]string{
				"first":  {"mac_address": "02:00:00:00:00:01", "ip": "192.0.2.10"},
				"second": {"mac_address": "02:00:00:00:00:02", "ip": "192.0.2.11"},
			}
			inventory["first"][test.field] = test.first
			inventory["second"][test.field] = test.second

			data, err := json.Marshal(inventory)
			if err != nil {
				t.Fatal(err)
			}

			path := t.TempDir() + "/hosts.json"

			if err := os.WriteFile(path, data, 0o600); err != nil {
				t.Fatal(err)
			}

			_, err = LoadHostsConfig(path)
			if err == nil {
				t.Fatal("accepted duplicate host identities")
			}

			for _, detail := range []string{"first", "second", test.first} {
				if !strings.Contains(err.Error(), detail) {
					t.Fatalf("error %q does not identify conflict %q", err, detail)
				}
			}
		})
	}
}
