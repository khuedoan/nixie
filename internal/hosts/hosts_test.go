package hosts

import (
	"os"
	"strings"
	"testing"
)

func TestLoadHostsConfig(t *testing.T) {
	const first = `"first":{"mac_address":"02:00:00:00:00:01","ip":"192.0.2.1"}`
	for _, test := range []struct {
		name    string
		host    string
		wantErr string
	}{
		{"valid", `{"mac_address":"02-00-00-00-00-02","ip":"2001:0db8::2"}`, ""},
		{"missing MAC", `{"ip":"192.0.2.2"}`, "invalid MAC address"},
		{"invalid MAC", `{"mac_address":"bad","ip":"192.0.2.2"}`, "invalid MAC address"},
		{"duplicate MAC", `{"mac_address":"02-00-00-00-00-01","ip":"192.0.2.2"}`, "share MAC address"},
		{"missing IP", `{"mac_address":"02:00:00:00:00:02"}`, "invalid IP address"},
		{"empty IP", `{"mac_address":"02:00:00:00:00:02","ip":""}`, "invalid IP address"},
		{"invalid IP", `{"mac_address":"02:00:00:00:00:02","ip":"bad"}`, "invalid IP address"},
		{"duplicate IP", `{"mac_address":"02:00:00:00:00:02","ip":"::ffff:192.0.2.1"}`, "share IP address"},
	} {
		t.Run(test.name, func(t *testing.T) {
			path := t.TempDir() + "/hosts.json"
			data := `{` + first + `,"second":` + test.host + `}`
			if err := os.WriteFile(path, []byte(data), 0o444); err != nil {
				t.Fatal(err)
			}
			config, err := LoadHostsConfig(path)
			if test.wantErr != "" {
				if err == nil || !strings.Contains(err.Error(), test.wantErr) {
					t.Fatalf("LoadHostsConfig() error = %v, want %q", err, test.wantErr)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			host := config["second"]
			if host.MACAddress.String() != "02:00:00:00:00:02" || host.IP != "2001:db8::2" || host.GetState() != StateUnknown {
				t.Fatalf("unexpected host: %+v", host)
			}
		})
	}
}
