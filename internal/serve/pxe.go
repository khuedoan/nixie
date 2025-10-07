package serve

import (
	"bytes"
	"fmt"
	"io"
	"os"

	"code.khuedoan.com/nixie/internal/hosts"

	"github.com/charmbracelet/log"
	"go.universe.tf/netboot/out/ipxe"
	"go.universe.tf/netboot/pixiecore"
)

type PXEBooter struct {
	Kernel      string
	Initrd      string
	Init        string
	HostsConfig hosts.HostsConfig
}

func (b *PXEBooter) BootSpec(m pixiecore.Machine) (*pixiecore.Spec, error) {
	for _, hostConfig := range b.HostsConfig {
		if bytes.Equal(hostConfig.MACAddress, m.MAC) {
			return &pixiecore.Spec{
				Kernel:  pixiecore.ID("kernel"),
				Initrd:  []pixiecore.ID{"initrd"},
				Cmdline: fmt.Sprintf("init=%s loglevel=4 nixie_mac_address=%s", b.Init, m.MAC),
			}, nil
		}
	}
	return nil, fmt.Errorf("unknown MAC address: %s", m.MAC)
}

func (b *PXEBooter) ReadBootFile(id pixiecore.ID) (io.ReadCloser, int64, error) {
	var path string
	switch string(id) {
	case "kernel":
		path = b.Kernel
	case "initrd":
		path = b.Initrd
	default:
		return nil, -1, fmt.Errorf("unknown file ID: %s", id)
	}

	f, err := os.Open(path)
	if err != nil {
		return nil, -1, err
	}

	stat, err := f.Stat()
	if err != nil {
		if closeErr := f.Close(); closeErr != nil {
			log.Debug("failed to close file after stat error", "error", closeErr)
		}
		return nil, -1, err
	}

	return f, stat.Size(), nil
}

func (b *PXEBooter) WriteBootFile(id pixiecore.ID, body io.Reader) error {
	return fmt.Errorf("WriteBootFile not supported")
}

func NewPXEServer(address, kernel, initrd, init string, hostsConfig hosts.HostsConfig) (*pixiecore.Server, error) {
	// TODO maybe build this with a new iPXE version with Nix
	efi64Data, err := ipxe.Asset("third_party/ipxe/src/bin-x86_64-efi/ipxe.efi")
	if err != nil {
		return nil, fmt.Errorf("failed to load embedded iPXE firmware: %w", err)
	}

	// Be defensive and check if the files exist
	for _, p := range []string{kernel, initrd, init} {
		if _, err := os.Stat(p); err != nil {
			return nil, fmt.Errorf("missing installer file: %s", p)
		}
	}

	ipxe := map[pixiecore.Firmware][]byte{
		// https://www.rfc-editor.org/errata_search.php?rfc=4578
		// Only FirmwareEFI64 is supported for now, FirmwareBC may be added later if needed
		// https://github.com/danderson/netboot/pull/30
		pixiecore.FirmwareEFI64: efi64Data,
	}

	booter := &PXEBooter{
		Kernel:      kernel,
		Initrd:      initrd,
		Init:        init,
		HostsConfig: hostsConfig,
	}

	server := &pixiecore.Server{
		Address:    address,
		Booter:     booter,
		DHCPNoBind: true,
		Ipxe:       ipxe,
		Log: func(subsystem, msg string) {
			log.Info(msg, "subsystem", subsystem)
		},
		Debug: func(subsystem, msg string) {
			log.Debug(msg, "subsystem", subsystem)
		},
	}

	return server, nil
}
