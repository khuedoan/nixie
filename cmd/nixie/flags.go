package main

import (
	"errors"
	"flag"
	"os"
)

type Flags struct {
	Address   string
	Debug     bool
	Flake     string
	HostsFile string
	Installer string
}

func parseFlags() (*Flags, error) {
	var flags Flags

	flag.BoolVar(&flags.Debug, "debug", false, "Enable debug logging")
	flag.StringVar(&flags.Address, "address", "", "Provisioning IP address to listen on (default auto)")
	flag.StringVar(&flags.Flake, "flake", "", "NixOS configuration flake (for example, .)")
	flag.StringVar(&flags.HostsFile, "hosts", "", "Read-only host inventory with fixed IP addresses (for example, ./hosts.json)")
	flag.StringVar(&flags.Installer, "installer", "", "NixOS installer flake output (for example, .#nixosConfigurations.installer)")

	if err := flag.CommandLine.Parse(os.Args[1:]); err != nil {
		return nil, err
	}

	if flags.HostsFile == "" || flags.Flake == "" || flags.Installer == "" {
		return nil, errors.New("missing flags, usage: nixie [--address <ip>] --hosts <hosts.json> --flake <flake> --installer <installer-output>")
	}

	return &flags, nil
}
