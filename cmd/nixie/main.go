package main

import (
	"flag"

	"github.com/charmbracelet/log"
)

type Config struct {
	Address   string
	Debug     bool
	Flake     string
	HostsFile string
	Installer string
	SSHKey    string
}

func parseFlags() Config {
	var config Config

	flag.BoolVar(&config.Debug, "debug", false, "Enable debug logging")
	flag.StringVar(&config.Address, "address", "0.0.0.0", "Address to listen on")
	flag.StringVar(&config.Flake, "flake", "", "NixOS configuration flake (for example, .)")
	flag.StringVar(&config.HostsFile, "hosts", "", "Path to hosts.json file (for example, ./hosts.json)")
	flag.StringVar(&config.Installer, "installer", "", "NixOS installer flake output (for example, .#installer)")
	flag.StringVar(&config.SSHKey, "ssh-key", "", "Path to the SSH private key (for example, ~/.ssh/id_ed25519)")

	flag.Parse()

	if config.HostsFile == "" || config.Flake == "" || config.Installer == "" {
		log.Fatal("Usage: nixie --hosts <hosts.json> --flake <flake> --installer <installer-output>")
	}

	return config
}

func setupLogging(debug bool) {
	if debug {
		log.SetLevel(log.DebugLevel)
	}
}

func main() {
	config := parseFlags()
	setupLogging(config.Debug)
	log.Debug("Parsed config", "config", config)
}
