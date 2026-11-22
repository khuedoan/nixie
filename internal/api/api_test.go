package api

import (
	"context"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"code.khuedoan.com/nixie/internal/hosts"
)

func TestInstallUsesConfiguredFinalAddress(t *testing.T) {
	directory := t.TempDir()
	for _, name := range []string{"nixos-anywhere", "ssh"} {
		script := "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NIXIE_TEST_DIR/" + name + ".args\"\n"
		if name == "ssh" {
			script += "printf '%s\\n' 0123456789abcdef0123456789abcdef\n"
		}
		if err := os.WriteFile(filepath.Join(directory, name), []byte(script), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	t.Setenv("NIXIE_TEST_DIR", directory)
	t.Setenv("PATH", directory+string(os.PathListSeparator)+os.Getenv("PATH"))
	inventory := filepath.Join(directory, "hosts.json")
	if err := os.WriteFile(inventory, []byte(`{"node":{"mac_address":"02:00:00:00:00:01","ip":"192.0.2.10"}}`), 0o644); err != nil {
		t.Fatal(err)
	}
	hostsConfig, err := hosts.LoadHostsConfig(inventory)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	api := &API{
		ctx:               ctx,
		hostsConfig:       hostsConfig,
		flake:             ".",
		deploymentSSHUser: "admin",
		sshAgentSocket:    "/test/agent.sock",
		doneCh:            make(chan struct{}, 1),
	}
	request := httptest.NewRequest(http.MethodPost, "/install", strings.NewReader(`{"mac_address":"02:00:00:00:00:01"}`))
	request.RemoteAddr = "192.0.2.20:12345"
	response := httptest.NewRecorder()
	api.install(response, request)
	if response.Code != http.StatusAccepted {
		t.Fatalf("install status = %d, want %d", response.Code, http.StatusAccepted)
	}
	select {
	case <-api.doneCh:
	case <-ctx.Done():
		t.Fatal("installation did not complete")
	}
	for command, target := range map[string]string{
		"nixos-anywhere": "root@192.0.2.20",
		"ssh":            "admin@192.0.2.10",
	} {
		data, err := os.ReadFile(filepath.Join(directory, command+".args"))
		if err != nil {
			t.Fatal(err)
		}
		if !slices.Contains(strings.Split(strings.TrimSpace(string(data)), "\n"), target) {
			t.Errorf("%s did not target %s: %s", command, target, data)
		}
	}
}
