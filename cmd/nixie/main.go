package main

import (
	"context"
	"net"
	"os"
	"os/signal"
	"sync"
	"syscall"
	"time"

	"code.khuedoan.com/nixie/internal/api"
	"code.khuedoan.com/nixie/internal/hosts"
	"code.khuedoan.com/nixie/internal/network"
	"code.khuedoan.com/nixie/internal/nixos"
	"code.khuedoan.com/nixie/internal/pxe"

	"github.com/charmbracelet/log"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

func main() {
	flags, err := parseFlags()
	if err != nil {
		log.Fatal("failed to parse command-line flags", "error", err)
	}

	if flags.Debug {
		log.SetLevel(log.DebugLevel)
	}

	log.Debug("parsed command line flags", "flags", flags)

	hostsConfig, err := hosts.LoadHostsConfig(flags.HostsFile)
	if err != nil {
		log.Fatal("failed to load hosts config", "error", err)
	}
	log.Debug("parsed hosts config", "hosts", hostsConfig)

	var address string
	if flags.Address == "" {
		address, err = network.DetectServerAddress()
		if err != nil {
			log.Fatal("failed to detect server address, please specify --address manually", "error", err)
		}
	} else {
		address = flags.Address
	}
	log.Debug("detected server IP", "address", address)

	ctx := context.Background()
	shutdownTrace, err := setupTracing(ctx, "nixie")
	if err != nil {
		log.Fatal("failed to start telemetry", "error", err)
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
	defer runSpan.End()

	ctx, cancel := context.WithCancel(ctx)
	defer cancel()

	checkReachableHosts(ctx, hostsConfig)
	if hosts.AllInstalled(hostsConfig) {
		log.Info("all hosts are already installed")
		return
	}

	log.Info("building installer", "installer", flags.Installer)
	installerComponents, err := nixos.BuildInstaller(ctx, flags.Installer, flags.Debug)
	if err != nil {
		runSpan.SetStatus(codes.Error, err.Error())
		runSpan.RecordError(err)
		log.Fatal("failed to build the installer", "error", err)
	}
	log.Debug("installer components", "kernel", installerComponents.Kernel, "initrd", installerComponents.Initrd, "init", installerComponents.Init)

	pxeServer, err := pxe.NewPXEServer(
		ctx,
		address,
		installerComponents.Kernel,
		installerComponents.Initrd,
		installerComponents.Init,
		hostsConfig,
	)
	if err != nil {
		runSpan.SetStatus(codes.Error, err.Error())
		runSpan.RecordError(err)
		log.Fatal("failed to create PXE server", "error", err)
	}

	pxeErrCh := make(chan error, 1)
	go func() {
		pxeErrCh <- pxeServer.Serve()
	}()
	log.Info("PXE server started", "address", address)

	doneCh := make(chan struct{}, 1)
	go func() {
		if err := api.StartAPIServer(ctx, hostsConfig, flags.Flake, flags.InstallSSHKey, flags.SSHAgentSocket, flags.Debug, doneCh); err != nil {
			log.Fatal("failed to start API server", "error", err)
		}
	}()

	// TODO probably need a better place to put these, maybe one go routine to manage each machine
	for name, host := range hostsConfig {
		if host.GetState() == hosts.StateInstalled {
			log.Debug("skipping installed host", "host", name, "mac", host.MACAddress)
			continue
		}
		log.Info("sending magic packet", "mac", host.MACAddress)
		mac, _ := net.ParseMAC(host.MACAddress)
		network.SendWakeOnLAN(mac)
	}

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)

	select {
	case sig := <-sigCh:
		log.Info("signal received, shutting down", "signal", sig)
	case <-doneCh:
		log.Info("all hosts installed, shutting down")
	case err := <-pxeErrCh:
		if err != nil {
			runSpan.SetStatus(codes.Error, err.Error())
			runSpan.RecordError(err)
			log.Fatal("PXE server stopped unexpectedly", "error", err)
		}
		log.Fatal("PXE server stopped unexpectedly")
	}

	pxeServer.Shutdown()
	select {
	case err := <-pxeErrCh:
		if err != nil {
			log.Warn("PXE server stopped with error", "error", err)
		}
	case <-time.After(5 * time.Second):
		log.Warn("timed out waiting for PXE server to stop")
	}

	log.Info("nixie stopped gracefully")
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
