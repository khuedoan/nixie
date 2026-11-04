package main

import (
	"errors"
	"flag"
	"os"
)

type Flags struct {
	Address        string
	Debug          bool
	Flake          string
	HostsFile      string
	InstallSSHKey  string
	Installer      string
	SSHAgentSocket string
}

func parseFlags() (*Flags, error) {
	var flags Flags

	flag.BoolVar(&flags.Debug, "debug", false, "Enable debug logging")
	flag.StringVar(&flags.Address, "address", "", "Address to listen on (default auto)")
	flag.StringVar(&flags.Flake, "flake", "", "NixOS configuration flake (for example, .)")
	flag.StringVar(&flags.HostsFile, "hosts", "", "Path to hosts.json file (for example, ./hosts.json)")
	flag.StringVar(&flags.InstallSSHKey, "install-ssh-key", "", "Path to the SSH private key authorized by the installer")
	flag.StringVar(&flags.Installer, "installer", "", "NixOS installer flake output (for example, .#nixosConfigurations.installer)")
	flag.StringVar(&flags.SSHAgentSocket, "ssh-agent-socket", os.Getenv("SSH_AUTH_SOCK"), "SSH agent socket (defaults to SSH_AUTH_SOCK); allows omitting the installer key flag")

	flag.Parse()

	if flags.HostsFile == "" || flags.Flake == "" || flags.Installer == "" {
		return nil, errors.New("missing flags, usage: nixie --hosts <hosts.json> --flake <flake> --installer <installer-output> [--ssh-agent-socket <socket> | --install-ssh-key <private-key>]")
	}
	if flags.SSHAgentSocket == "" && flags.InstallSSHKey == "" {
		return nil, errors.New("SSH authentication requires SSH_AUTH_SOCK, --ssh-agent-socket, or --install-ssh-key")
	}

	return &flags, nil
}
