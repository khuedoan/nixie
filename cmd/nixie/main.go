package main

import (
	"context"
	"os"
	"os/signal"
	"syscall"

	"code.khuedoan.com/nixie/internal/hosts"
	"code.khuedoan.com/nixie/internal/network"
	"code.khuedoan.com/nixie/internal/nixos"
	"code.khuedoan.com/nixie/internal/serve"

	"github.com/charmbracelet/log"
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
			log.Fatal("failed to detect server address, please specify --address manually")
		}
	} else {
		address = flags.Address
	}
	log.Debug("detected server IP", "address", address)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	log.Info("building installer", "installer", flags.Installer)
	installerComponents, err := nixos.BuildInstaller(ctx, flags.Installer, flags.Debug)
	if err != nil {
		log.Fatal("failed to build the installer", "error", err)
	}
	log.Debug("installer components", "kernel", installerComponents.Kernel, "initrd", installerComponents.Initrd, "init", installerComponents.Init)

	pxeServer, err := serve.NewPXEServer(
		address,
		installerComponents.Kernel,
		installerComponents.Initrd,
		installerComponents.Init,
		hostsConfig,
	)
	if err != nil {
		log.Fatal("failed to create PXE server", "error", err)
	}

	go func() {
		if err := pxeServer.Serve(); err != nil {
			log.Fatal("failed to start PXE server", "error", err)
		}
	}()
	log.Info("PXE server started", "address", address)

	go func() {
		if err := serve.StartAPIServer(ctx, flags.Debug); err != nil {
			log.Fatal("failed to start API server", "error", err)
		}
	}()
	log.Info("API server started", "address", address)

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)

	sig := <-sigCh
	log.Info("signal received, shutting down", "signal", sig)
	pxeServer.Shutdown()

	log.Info("nixie stopped gracefully")
}
