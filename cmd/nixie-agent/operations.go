package main

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/exec"
	"strconv"
	"strings"

	"code.khuedoan.com/nixie/internal/protocol"
)

func installFlake(ctx context.Context, flake string, reporter statusReporter) error {
	if err := reporter.send(ctx, protocol.InstallResult{Status: protocol.StatusStarted}); err != nil {
		return err
	}

	disko, err := buildDisko(ctx, flake)
	if err != nil {
		return fmt.Errorf("build disko script: %w", err)
	}

	err = runLocal(ctx, disko)
	if err != nil {
		return fmt.Errorf("partition target disks: %w", err)
	}

	err = runLocal(ctx, "nixos-install",
		"--flake", flake,
		"--root", "/mnt",
		"--no-root-passwd",
		"--no-channel-copy",
		"--no-write-lock-file",
	)
	if err != nil {
		return err
	}

	if err := reporter.send(ctx, protocol.InstallResult{Status: protocol.StatusCompleted}); err != nil {
		return err
	}

	err = scheduleReboot(ctx)
	return err
}

func buildDisko(ctx context.Context, flake string) (string, error) {
	source, configuration, ok := strings.Cut(flake, "#")
	if !ok || source == "" || configuration == "" {
		return "", fmt.Errorf("flake must include a configuration fragment")
	}

	output := source + "#nixosConfigurations." + strconv.Quote(configuration) + ".config.system.build.diskoScript"
	cmd := exec.CommandContext(ctx, "nix", "build",
		"--extra-experimental-features", "nix-command flakes",
		"--no-write-lock-file",
		"--no-link",
		"--print-out-paths",
		output,
	)

	var stdout bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = os.Stderr

	if err := cmd.Run(); err != nil {
		return "", fmt.Errorf("nix build: %w", err)
	}

	return strings.TrimSpace(stdout.String()), nil
}

func scheduleReboot(ctx context.Context) error {
	if zpool, err := exec.LookPath("zpool"); err == nil {
		cmd := exec.CommandContext(ctx, zpool, "list")
		cmd.Stderr = os.Stderr
		output, err := cmd.Output()
		if err != nil {
			return fmt.Errorf("list ZFS pools: %w", err)
		}

		if strings.TrimSpace(string(output)) != "no pools available" {
			if err := runLocal(ctx, "umount", "-R", "/mnt"); err != nil {
				return err
			}

			if err := runLocal(ctx, "swapoff", "-a"); err != nil {
				return err
			}

			_ = runLocal(ctx, zpool, "export", "-a")
		}
	}

	systemctl, err := exec.LookPath("systemctl")
	if err != nil {
		return fmt.Errorf("find systemctl: %w", err)
	}

	return runLocal(ctx, "systemd-run", "--on-active=5s", "--collect", systemctl, "reboot")
}

func runLocal(ctx context.Context, name string, args ...string) error {
	cmd := exec.CommandContext(ctx, name, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr

	if err := cmd.Run(); err != nil {
		return fmt.Errorf("%s: %w", name, err)
	}

	return nil
}
