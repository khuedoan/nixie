package main

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/url"
	"os"
	"os/signal"
	"path/filepath"
	"sync"
	"syscall"
	"time"

	"code.khuedoan.com/nixie/internal/api"
	"code.khuedoan.com/nixie/internal/hosts"
	"code.khuedoan.com/nixie/internal/network"
	"code.khuedoan.com/nixie/internal/nixos"
	"code.khuedoan.com/nixie/internal/pxe"
	"code.khuedoan.com/nixie/internal/telemetry"

	"github.com/charmbracelet/log"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/trace"
)

func main() {
	if err := run(); err != nil {
		log.Fatal("nixie failed", "error", err)
	}
}

func run() (runErr error) {
	flags, err := parseFlags()
	if err != nil {
		return fmt.Errorf("failed to parse command-line flags: %w", err)
	}

	if flags.Debug {
		log.SetLevel(log.DebugLevel)
	}

	log.Debug("parsed command line flags", "flags", flags)

	hostsConfig, err := hosts.LoadHostsConfig(flags.HostsFile)
	if err != nil {
		return fmt.Errorf("failed to load hosts config: %w", err)
	}

	log.Debug("parsed hosts config", "hosts", hostsConfig)

	address := flags.Address
	if address == "" {
		address, err = network.DetectServerAddress()
		if err != nil {
			return fmt.Errorf("detect provisioning address; specify --address explicitly: %w", err)
		}
	}

	log.Debug("provisioning address", "address", address)

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	shutdownTrace, err := telemetry.Setup(ctx)
	if err != nil {
		return fmt.Errorf("failed to start telemetry: %w", err)
	}

	defer func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()

		if err := shutdownTrace(ctx); err != nil {
			log.Warn("failed to shutdown telemetry", "error", err)
		}
	}()

	ctx, runSpan := otel.Tracer("nixie").Start(ctx, "nixie.run", trace.WithAttributes(
		attribute.String("nixie.installer", flags.Installer),
		attribute.String("nixie.flake", flags.Flake),
		attribute.String("nixie.hosts_file", flags.HostsFile),
	))
	defer func() { telemetry.End(runSpan, runErr) }()

	ctx, cancel := context.WithCancel(ctx)
	defer cancel()

	checkReachableHosts(ctx, hostsConfig)

	if err := ctx.Err(); err != nil {
		return err
	}

	pending := 0

	for _, host := range hostsConfig {
		if host.GetState() != hosts.StateInstalled {
			pending++
		}
	}

	if pending == 0 {
		log.Info("all hosts have reachable SSH, skipping provisioning")
		return nil
	}

	sourceDir, err := os.MkdirTemp("", "nixie-source-")
	if err != nil {
		return fmt.Errorf("create source directory: %w", err)
	}

	defer os.RemoveAll(sourceDir)

	sourceArchive := filepath.Join(sourceDir, "source.tar.gz")
	directory, err := nixos.ArchiveFlake(ctx, flags.Flake, sourceArchive)
	if err != nil {
		return fmt.Errorf("snapshot flake source: %w", err)
	}

	sourceURL := &url.URL{
		Scheme: "http",
		Host:   net.JoinHostPort(address, "5000"),
		Path:   "/source.tar.gz",
	}

	if directory != "" {
		sourceURL.RawQuery = url.Values{"dir": {directory}}.Encode()
	}

	log.Info("building installer", "installer", flags.Installer)
	installerComponents, err := nixos.BuildInstaller(ctx, flags.Installer, flags.Debug)
	if err != nil {
		return fmt.Errorf("failed to build the installer: %w", err)
	}

	log.Debug("installer components",
		"kernel", installerComponents.Kernel,
		"initrd", installerComponents.Initrd,
		"init", installerComponents.Init,
	)

	pxeServer, err := pxe.NewPXEServer(
		address,
		installerComponents.Kernel,
		installerComponents.Initrd,
		installerComponents.Init,
		sourceURL.String(),
		hostsConfig,
	)
	if err != nil {
		return fmt.Errorf("failed to create PXE server: %w", err)
	}

	results := make(chan error, pending)
	serverErrors := make(chan error, 2)
	var servers sync.WaitGroup
	servers.Add(2)

	go func() {
		defer servers.Done()
		serverErrors <- pxeServer.Serve(ctx)
	}()

	go func() {
		defer servers.Done()
		serverErrors <- api.StartAPIServer(ctx, hostsConfig, address, sourceArchive, results)
	}()

	for name, host := range hostsConfig {
		if host.GetState() == hosts.StateInstalled {
			log.Debug("skipping installed host", "host", name, "mac", host.MACAddress)
			continue
		}

		log.Info("sending magic packet", "mac", host.MACAddress)
		mac, _ := net.ParseMAC(host.MACAddress)
		network.SendWakeOnLAN(mac)
	}

	runErr = waitForHosts(ctx, pending, results, serverErrors)

	cancel()
	stopped := make(chan struct{})
	go func() {
		servers.Wait()
		close(stopped)
	}()

	select {
	case <-stopped:
	case <-time.After(10 * time.Second):
		return errors.Join(runErr, errors.New("timed out stopping servers"))
	}

	log.Info("nixie stopped")
	return runErr
}

func waitForHosts(ctx context.Context, pending int, results, serverErrors <-chan error) error {
	var resultErr error

	for pending > 0 {
		select {
		case <-ctx.Done():
			return errors.Join(resultErr, ctx.Err())

		case err := <-serverErrors:
			if ctx.Err() != nil {
				return errors.Join(resultErr, ctx.Err(), err)
			}

			if err == nil {
				err = errors.New("server stopped unexpectedly")
			}

			return errors.Join(resultErr, err)

		case err := <-results:
			resultErr = errors.Join(resultErr, err)
			pending--
		}
	}

	return resultErr
}

func checkReachableHosts(ctx context.Context, hostsConfig hosts.HostsConfig) {
	var wg sync.WaitGroup

	for name, host := range hostsConfig {
		wg.Add(1)
		go func() {
			defer wg.Done()

			checkCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
			defer cancel()

			fingerprint, err := nixos.ReadHostFingerprint(checkCtx, host.IP)
			if err != nil {
				return
			}

			host.SetState(hosts.StateInstalled)
			log.Info("SSH reachable, skipping installation",
				"host", name,
				"ip", host.IP,
				"fingerprint", fingerprint,
			)
		}()
	}

	wg.Wait()
}
