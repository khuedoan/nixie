package main

import (
	"flag"
	"os"
	"testing"
)

func TestParseFlagsValidatesSSHAuthentication(t *testing.T) {
	tests := []struct {
		name              string
		installKey        string
		deploymentKey     string
		environmentSocket string
		agentSocket       string
		wantSocket        string
		wantAccepted      bool
	}{
		{name: "missing authentication"},
		{name: "installer key only", installKey: "/keys/installer"},
		{name: "deployment key only", deploymentKey: "/keys/deployment"},
		{name: "both keys", installKey: "/keys/installer", deploymentKey: "/keys/deployment", wantAccepted: true},
		{name: "environment agent", environmentSocket: "/run/user/1000/environment-agent.sock", wantSocket: "/run/user/1000/environment-agent.sock", wantAccepted: true},
		{name: "agent only", agentSocket: "/run/user/1000/ssh-agent.sock", wantSocket: "/run/user/1000/ssh-agent.sock", wantAccepted: true},
		{name: "explicit agent overrides environment", environmentSocket: "/run/user/1000/environment-agent.sock", agentSocket: "/run/user/1000/explicit-agent.sock", wantSocket: "/run/user/1000/explicit-agent.sock", wantAccepted: true},
		{name: "agent and installer key", installKey: "/keys/installer", agentSocket: "/run/user/1000/ssh-agent.sock", wantSocket: "/run/user/1000/ssh-agent.sock", wantAccepted: true},
		{name: "agent and deployment key", deploymentKey: "/keys/deployment", agentSocket: "/run/user/1000/ssh-agent.sock", wantSocket: "/run/user/1000/ssh-agent.sock", wantAccepted: true},
		{name: "agent and both keys", installKey: "/keys/installer", deploymentKey: "/keys/deployment", agentSocket: "/run/user/1000/ssh-agent.sock", wantSocket: "/run/user/1000/ssh-agent.sock", wantAccepted: true},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			t.Setenv("SSH_AUTH_SOCK", test.environmentSocket)
			args := []string{
				"--hosts", "hosts.json",
				"--flake", ".",
				"--installer", ".#nixosConfigurations.installer",
			}
			if test.installKey != "" {
				args = append(args, "--install-ssh-key", test.installKey)
			}
			if test.deploymentKey != "" {
				args = append(args, "--deployment-ssh-key", test.deploymentKey)
			}
			if test.agentSocket != "" {
				args = append(args, "--ssh-agent-socket", test.agentSocket)
			}

			flags, err := parseFlagsForTest(t, args...)
			if !test.wantAccepted {
				if err == nil || err.Error() != "SSH authentication requires SSH_AUTH_SOCK, --ssh-agent-socket, or both --install-ssh-key and --deployment-ssh-key" {
					t.Fatalf("parseFlags() error = %v", err)
				}
				if flags != nil {
					t.Fatalf("parseFlags() flags = %+v, want nil", flags)
				}
				return
			}

			if err != nil {
				t.Fatalf("parseFlags() error = %v", err)
			}
			want := &Flags{
				DeploymentSSHKey:  test.deploymentKey,
				DeploymentSSHUser: "root",
				Flake:             ".",
				HostsFile:         "hosts.json",
				InstallSSHKey:     test.installKey,
				Installer:         ".#nixosConfigurations.installer",
				SSHAgentSocket:    test.wantSocket,
			}
			if *flags != *want {
				t.Fatalf("parseFlags() = %+v, want %+v", flags, want)
			}
		})
	}
}

func TestParseFlagsRequiresDeploymentInputs(t *testing.T) {
	tests := []struct {
		name      string
		hostsFile string
		flake     string
		installer string
	}{
		{name: "missing hosts file", flake: ".", installer: ".#nixosConfigurations.installer"},
		{name: "missing flake", hostsFile: "hosts.json", installer: ".#nixosConfigurations.installer"},
		{name: "missing installer", hostsFile: "hosts.json", flake: "."},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			t.Setenv("SSH_AUTH_SOCK", "")
			args := []string{"--ssh-agent-socket", "/run/user/1000/ssh-agent.sock"}
			if test.hostsFile != "" {
				args = append(args, "--hosts", test.hostsFile)
			}
			if test.flake != "" {
				args = append(args, "--flake", test.flake)
			}
			if test.installer != "" {
				args = append(args, "--installer", test.installer)
			}

			flags, err := parseFlagsForTest(t, args...)
			if err == nil || err.Error() != "missing flags, usage: nixie --hosts <hosts.json> --flake <flake> --installer <installer-output> [--ssh-agent-socket <socket> | --install-ssh-key <private-key> --deployment-ssh-key <private-key>]" {
				t.Fatalf("parseFlags() error = %v", err)
			}
			if flags != nil {
				t.Fatalf("parseFlags() flags = %+v, want nil", flags)
			}
		})
	}
}

func parseFlagsForTest(t *testing.T, args ...string) (*Flags, error) {
	t.Helper()

	originalArgs := os.Args
	originalCommandLine := flag.CommandLine
	t.Cleanup(func() {
		os.Args = originalArgs
		flag.CommandLine = originalCommandLine
	})

	flag.CommandLine = flag.NewFlagSet("nixie", flag.ContinueOnError)
	os.Args = append([]string{"nixie"}, args...)
	return parseFlags()
}
