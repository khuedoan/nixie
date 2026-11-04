package pxe

import (
	"context"
	"fmt"
	"io"
	"net"
	"net/url"
	"os"

	"code.khuedoan.com/nixie/internal/hosts"

	"github.com/charmbracelet/log"
	"go.universe.tf/netboot/out/ipxe"
	"go.universe.tf/netboot/pixiecore"
)

type Server struct {
	server *pixiecore.Server
}

type PXEBooter struct {
	Address     string
	Kernel      string
	Initrd      string
	Init        string
	Flake       string
	HostsConfig hosts.HostsConfig
}

func (b *PXEBooter) BootSpec(m pixiecore.Machine) (*pixiecore.Spec, error) {
	for flakeOutput, hostConfig := range b.HostsConfig {
		if hostConfig.MACAddress == m.MAC.String() {
			if hostConfig.GetState() != hosts.StateUnknown {
				return nil, fmt.Errorf("PXE boot already used for MAC address: %s", m.MAC)
			}

			return &pixiecore.Spec{
				Kernel: pixiecore.ID("kernel"),
				Initrd: []pixiecore.ID{"initrd"},
				Cmdline: fmt.Sprintf(
					"init=%s loglevel=4 console=tty0 console=ttyS0,115200 "+
						"nixie_mac_address=%s nixie_api=%s nixie_flake=%s",
					b.Init,
					url.QueryEscape(m.MAC.String()),
					url.QueryEscape(net.JoinHostPort(b.Address, "5000")),
					url.QueryEscape(b.Flake+"#"+flakeOutput),
				),
			}, nil
		}
	}

	return nil, fmt.Errorf("unknown MAC address: %s", m.MAC)
}

func (b *PXEBooter) ReadBootFile(id pixiecore.ID) (file io.ReadCloser, size int64, err error) {
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

func (b *PXEBooter) WriteBootFile(_ pixiecore.ID, _ io.Reader) error {
	return fmt.Errorf("WriteBootFile not supported")
}

func (s *Server) Serve(ctx context.Context) error {
	if ctx.Err() != nil {
		return nil
	}

	var stopShutdown func() bool
	debug := s.server.Debug
	s.server.Debug = func(subsystem, msg string) {
		// The pinned Pixiecore version initializes its shutdown channel here.
		// It has no explicit startup hook, so register cancellation at this point.
		if subsystem == "Init" && msg == "Starting Pixiecore goroutines" {
			stopShutdown = context.AfterFunc(ctx, s.server.Shutdown)
		}

		debug(subsystem, msg)
	}

	defer func() {
		if stopShutdown != nil {
			stopShutdown()
		}
	}()

	return s.server.Serve()
}

func NewPXEServer(
	address, kernel, initrd, init, flake string,
	hostsConfig hosts.HostsConfig,
) (*Server, error) {
	efi64Data, err := ipxe.Asset("third_party/ipxe/src/bin-x86_64-efi/ipxe.efi")
	if err != nil {
		return nil, fmt.Errorf("failed to load embedded iPXE firmware: %w", err)
	}

	for _, p := range []string{kernel, initrd, init} {
		if _, err := os.Stat(p); err != nil {
			return nil, fmt.Errorf("missing installer file: %s", p)
		}
	}

	ipxe := map[pixiecore.Firmware][]byte{
		// https://www.rfc-editor.org/errata_search.php?rfc=4578
		// https://github.com/danderson/netboot/pull/30
		pixiecore.FirmwareEFI64: efi64Data,
	}

	booter := &PXEBooter{
		Address:     address,
		Kernel:      kernel,
		Initrd:      initrd,
		Init:        init,
		Flake:       flake,
		HostsConfig: hostsConfig,
	}

	pixiecoreServer := &pixiecore.Server{
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

	return &Server{
		server: pixiecoreServer,
	}, nil
}
