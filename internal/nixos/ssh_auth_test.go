package nixos

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const (
	testMachineID     = "0123456789abcdef0123456789abcdef"
	testMachineIDHash = "dada2cfcc22d3f6285b65cf851d8582adeeb8d70f7f0fcfe9402cbef732b23ec"
)

func TestInstallUsesSSHAgentWithoutIdentityFile(t *testing.T) {
	captureFile := installFakeCommand(t, "nixos-anywhere", "exit 0")
	socket := "/run/user/1000/ssh-agent.sock"

	if err := Install(context.Background(), ".#claw", "root", "192.0.2.1", "", socket, false); err != nil {
		t.Fatalf("Install() error = %v", err)
	}

	assertSSHInvocation(t, captureFile, socket, "")
}

func TestReadMachineIDUsesSSHAgentWithoutIdentityFile(t *testing.T) {
	captureFile := installFakeCommand(t, "ssh", `printf '%s\n' `+testMachineID)
	socket := "/run/user/1000/ssh-agent.sock"

	machineIDHash, err := ReadMachineIDHash(context.Background(), "root", "192.0.2.1", "", socket, false)
	if err != nil {
		t.Fatalf("ReadMachineIDHash() error = %v", err)
	}
	if machineIDHash != testMachineIDHash {
		t.Fatalf("ReadMachineIDHash() = %q, want %q", machineIDHash, testMachineIDHash)
	}

	assertSSHInvocation(t, captureFile, socket, "")
}

func TestCommandsUseSSHKey(t *testing.T) {
	tests := []struct {
		name          string
		command       string
		commandResult string
		run           func(context.Context, string) (string, error)
		wantResult    string
	}{
		{
			name:          "install",
			command:       "nixos-anywhere",
			commandResult: "exit 0",
			run: func(ctx context.Context, key string) (string, error) {
				return "", Install(ctx, ".#claw", "root", "192.0.2.1", key, "", false)
			},
		},
		{
			name:          "read machine ID",
			command:       "ssh",
			commandResult: `printf '%s\n' ` + testMachineID,
			run: func(ctx context.Context, key string) (string, error) {
				return ReadMachineIDHash(ctx, "root", "192.0.2.1", key, "", false)
			},
			wantResult: testMachineIDHash,
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			t.Setenv("SSH_AUTH_SOCK", "")
			captureFile := installFakeCommand(t, test.command, test.commandResult)
			key := "/keys/id_ed25519"

			result, err := test.run(context.Background(), key)
			if err != nil {
				t.Fatalf("command error = %v", err)
			}
			if result != test.wantResult {
				t.Fatalf("command result = %q, want %q", result, test.wantResult)
			}

			assertSSHInvocation(t, captureFile, "", key)
		})
	}
}

func installFakeCommand(t *testing.T, name, command string) string {
	t.Helper()

	directory := t.TempDir()
	captureFile := filepath.Join(directory, "invocation")
	script := "#!/bin/sh\n" +
		"printf '%s\\n' \"$SSH_AUTH_SOCK\" > \"$CAPTURE_FILE\"\n" +
		"printf '%s\\n' \"$@\" >> \"$CAPTURE_FILE\"\n" +
		command + "\n"
	if err := os.WriteFile(filepath.Join(directory, name), []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}

	t.Setenv("CAPTURE_FILE", captureFile)
	t.Setenv("PATH", directory+string(os.PathListSeparator)+os.Getenv("PATH"))
	return captureFile
}

func assertSSHInvocation(t *testing.T, captureFile, socket, key string) {
	t.Helper()

	invocation, err := os.ReadFile(captureFile)
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimSuffix(string(invocation), "\n"), "\n")
	if lines[0] != socket {
		t.Fatalf("SSH_AUTH_SOCK = %q, want %q", lines[0], socket)
	}

	arguments := lines[1:]
	foundIdentityFile := false
	identityFile := ""
	for index, argument := range arguments {
		if argument != "-i" {
			continue
		}
		foundIdentityFile = true
		if index+1 < len(arguments) {
			identityFile = arguments[index+1]
		}
	}
	if key == "" && foundIdentityFile {
		t.Fatalf("agent-only invocation contains -i: %q", arguments)
	}
	if key != "" && !foundIdentityFile {
		t.Fatalf("invocation does not contain -i %q: %q", key, arguments)
	}
	if identityFile != key {
		t.Fatalf("identity file = %q, want %q; arguments = %q", identityFile, key, arguments)
	}
}
