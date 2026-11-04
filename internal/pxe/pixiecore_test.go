package pxe

import (
	"net"
	"net/url"
	"strings"
	"testing"

	"code.khuedoan.com/nixie/internal/hosts"
	"go.universe.tf/netboot/pixiecore"
)

func TestBootSpecCarriesCompleteEncodedFlakeReference(t *testing.T) {
	mac, err := net.ParseMAC("02:00:00:00:00:01")
	if err != nil {
		t.Fatal(err)
	}

	host := &hosts.Host{MACAddress: mac.String()}
	booter := &PXEBooter{
		ctx:     t.Context(),
		Address: "fd00::1", Init: "/nix/store/installer+source/init",
		Flake:       "http://controller/source.tar.gz?dir=example%20directory&rev=a+b",
		HostsConfig: hosts.HostsConfig{"node with spaces": host},
	}
	machine := pixiecore.Machine{MAC: mac}

	for range 2 {
		spec, err := booter.BootSpec(machine)
		if err != nil {
			t.Fatal(err)
		}

		fields := strings.Fields(spec.Cmdline)
		if fields[0] != "init=/nix/store/installer+source/init" {
			t.Fatalf("kernel init path was escaped: %s", fields[0])
		}

		values, err := url.ParseQuery(strings.Join(fields[1:], "&"))
		if err != nil {
			t.Fatal(err)
		}

		if values.Get("nixie_flake") != "http://controller/source.tar.gz?dir=example%20directory&rev=a+b#node with spaces" ||
			values.Get("nixie_api") != "[fd00::1]:5000" ||
			values.Get("nixie_mac_address") != "02:00:00:00:00:01" {
			t.Fatalf("decoded installer boot values = %+v", values)
		}
	}

	host.SetState(hosts.StateInstalling)

	if _, err := booter.BootSpec(machine); err == nil {
		t.Fatal("continued to PXE boot after installation started")
	}

	unknown, _ := net.ParseMAC("02:00:00:00:00:02")

	if _, err := booter.BootSpec(pixiecore.Machine{MAC: unknown}); err == nil {
		t.Fatal("issued a boot specification for an unknown host")
	}
}
