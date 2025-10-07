package main

import (
	"reflect"
	"testing"
)

func TestParseKernelParams(t *testing.T) {
	tests := []struct {
		name string
		in   string
		want map[string]string
	}{
		{
			"Valid",
			"kernel initrd=initrd0 init=/nix/store/nixos-installer-kexec/init loglevel=4 nixie_mac_address=bc:24:11:0d:2f:20 nixie_api=192.168.1.15:5000",
			map[string]string{
				"initrd":            "initrd0",
				"init":              "/nix/store/nixos-installer-kexec/init",
				"loglevel":          "4",
				"nixie_mac_address": "bc:24:11:0d:2f:20",
				"nixie_api":         "192.168.1.15:5000",
			},
		},
		{
			"OnlyMAC",
			"kernel nixie_mac_address=aa:bb:cc:dd:ee:ff",
			map[string]string{
				"nixie_mac_address": "aa:bb:cc:dd:ee:ff",
			},
		},
		{
			"OnlyAPI",
			"kernel nixie_api=192.168.1.100:5000",
			map[string]string{
				"nixie_api": "192.168.1.100:5000",
			},
		},
		{
			"Irrelevant",
			"kernel root=/dev/sda1 ro quiet",
			map[string]string{
				"root": "/dev/sda1",
			},
		},
		{
			"Malformed",
			"kernel nixie_api nixie_mac_address=aa:bb:cc:dd:ee:ff",
			map[string]string{
				"nixie_mac_address": "aa:bb:cc:dd:ee:ff",
			},
		},
		{
			"Empty",
			"",
			map[string]string{},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := parseKernelParams(tt.in)
			if !reflect.DeepEqual(got, tt.want) {
				t.Errorf("parseKernelParams(%v) = %+v, want %+v", tt.in, got, tt.want)
			}
		})
	}
}
