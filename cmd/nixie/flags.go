package main

import (
	"errors"
	"flag"
)

type Flags struct {
	Address           string
	Debug             bool
	DeploymentSSHKey  string
	DeploymentSSHUser string
	Flake             string
	HostsFile         string
	InstallSSHKey     string
	Installer         string
	SSHAgentSocket    string
}

func parseFlags() (*Flags, error) {
	var flags Flags

	flag.BoolVar(&flags.Debug, "debug", false, "Enable debug logging")
	flag.StringVar(&flags.Address, "address", "", "Address to listen on (default auto)")
	flag.StringVar(&flags.DeploymentSSHKey, "deployment-ssh-key", "", "Path to the SSH private key authorized by the installed system")
	flag.StringVar(&flags.DeploymentSSHUser, "deployment-ssh-user", "root", "SSH user for the installed system")
	flag.StringVar(&flags.Flake, "flake", "", "NixOS configuration flake (for example, .)")
	flag.StringVar(&flags.HostsFile, "hosts", "", "Path to hosts.json file (for example, ./hosts.json)")
	flag.StringVar(&flags.InstallSSHKey, "install-ssh-key", "", "Path to the SSH private key authorized by the installer")
	flag.StringVar(&flags.Installer, "installer", "", "NixOS installer flake output (for example, .#nixosConfigurations.installer)")
	flag.StringVar(&flags.SSHAgentSocket, "ssh-agent-socket", "", "Path to the SSH agent socket used for encrypted private keys")

	flag.Parse()

	if flags.HostsFile == "" || flags.Flake == "" || flags.Installer == "" || flags.InstallSSHKey == "" || flags.DeploymentSSHKey == "" {
		return nil, errors.New("missing flags, usage: nixie --hosts <hosts.json> --flake <flake> --installer <installer-output> --install-ssh-key <private-key> --deployment-ssh-key <private-key>")
	}

	return &flags, nil
}
