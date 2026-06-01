package pxe

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"os"

	"code.khuedoan.com/nixie/internal/hosts"

	"github.com/charmbracelet/log"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
	"go.universe.tf/netboot/out/ipxe"
	"go.universe.tf/netboot/pixiecore"
)

type Server struct {
	ctx    context.Context
	booter *PXEBooter
	server *pixiecore.Server
}

type PXEBooter struct {
	ctx         context.Context
	Address     string
	Kernel      string
	Initrd      string
	Init        string
	HostsConfig hosts.HostsConfig
}

func (b *PXEBooter) BootSpec(m pixiecore.Machine) (*pixiecore.Spec, error) {
	_, span := otel.Tracer("nixie").Start(b.ctx, "pxe.boot_spec", trace.WithAttributes(
		attribute.String("host.mac", m.MAC.String()),
		attribute.String("pxe.arch", m.Arch.String()),
	))
	defer span.End()

	for flakeOutput, hostConfig := range b.HostsConfig {
		if bytes.Equal(hostConfig.MACAddress, m.MAC) {
			span.SetAttributes(attribute.String("nix.flake_output", flakeOutput))
			if hostConfig.GetState() != hosts.StateUnknown {
				span.SetAttributes(
					attribute.Bool("pxe.accepted", false),
					attribute.String("pxe.reject_reason", "already_used"),
				)
				return nil, fmt.Errorf("PXE boot already used for MAC address: %s", m.MAC)
			}

			span.SetAttributes(
				attribute.Bool("pxe.accepted", true),
			)
			return &pixiecore.Spec{
				Kernel:  pixiecore.ID("kernel"),
				Initrd:  []pixiecore.ID{"initrd"},
				Cmdline: fmt.Sprintf("init=%s loglevel=4 nixie_mac_address=%s nixie_api=%s:5000", b.Init, m.MAC, b.Address),
			}, nil
		}
	}

	span.SetAttributes(
		attribute.Bool("pxe.accepted", false),
		attribute.String("pxe.reject_reason", "unknown_mac"),
	)
	return nil, fmt.Errorf("unknown MAC address: %s", m.MAC)
}

func (b *PXEBooter) ReadBootFile(id pixiecore.ID) (file io.ReadCloser, size int64, err error) {
	_, span := otel.Tracer("nixie").Start(b.ctx, "pxe.read_boot_file", trace.WithAttributes(
		attribute.String("pxe.file_id", string(id)),
	))
	defer func() {
		if err != nil {
			span.SetStatus(codes.Error, err.Error())
			span.RecordError(err)
		}
		span.End()
	}()

	var path string
	switch string(id) {
	case "kernel":
		path = b.Kernel
	case "initrd":
		path = b.Initrd
	default:
		return nil, -1, fmt.Errorf("unknown file ID: %s", id)
	}
	span.SetAttributes(attribute.String("file.path", path))

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
	span.SetAttributes(attribute.Int64("file.size", stat.Size()))

	return f, stat.Size(), nil
}

func (b *PXEBooter) WriteBootFile(_ pixiecore.ID, _ io.Reader) error {
	return fmt.Errorf("WriteBootFile not supported")
}

func (s *Server) Serve() (err error) {
	ctx, span := otel.Tracer("nixie").Start(s.ctx, "pxe.serve", trace.WithAttributes(
		attribute.String("server.address", s.server.Address),
		attribute.Bool("pxe.dhcp_no_bind", s.server.DHCPNoBind),
	))
	defer func() {
		if err != nil {
			span.SetStatus(codes.Error, err.Error())
			span.RecordError(err)
		}
		span.End()
	}()

	s.booter.ctx = ctx
	return s.server.Serve()
}

func (s *Server) Shutdown() {
	s.server.Shutdown()
}

func NewPXEServer(ctx context.Context, address, kernel, initrd, init string, hostsConfig hosts.HostsConfig) (*Server, error) {
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

	if ctx == nil {
		ctx = context.Background()
	}

	booter := &PXEBooter{
		ctx:         ctx,
		Address:     address,
		Kernel:      kernel,
		Initrd:      initrd,
		Init:        init,
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
		ctx:    ctx,
		booter: booter,
		server: pixiecoreServer,
	}, nil
}
