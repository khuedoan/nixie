package main

import (
	"errors"
	"flag"
)

type Flags struct {
	Address   string
	Debug     bool
	Flake     string
	HostsFile string
	Installer string
	SSHKey    string
}

func parseFlags() (*Flags, error) {
	var flags Flags

	flag.BoolVar(&flags.Debug, "debug", false, "Enable debug logging")
	flag.StringVar(&flags.Address, "address", "", "Address to listen on (default auto)")
	flag.StringVar(&flags.Flake, "flake", "", "NixOS configuration flake (for example, .)")
	flag.StringVar(&flags.HostsFile, "hosts", "", "Path to hosts.json file (for example, ./hosts.json)")
	flag.StringVar(&flags.Installer, "installer", "", "NixOS installer flake output (for example, .#nixosConfigurations.installer)")
	flag.StringVar(&flags.SSHKey, "ssh-key", "", "Path to the SSH private key (for example, ~/.ssh/id_ed25519)")

	flag.Parse()

	if flags.HostsFile == "" || flags.Flake == "" || flags.Installer == "" {
		return nil, errors.New("missing flags, usage: nixie --hosts <hosts.json> --flake <flake> --installer <installer-output>")
	}

	return &flags, nil
}
