package nixos

import (
	"context"
	"fmt"
	"os"
	"os/exec"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

func Install(ctx context.Context, flakeRef, user, host, sshKey string, debug bool) (err error) {
	target := sshTarget(user, host)
	_, span := otel.Tracer("nixie").Start(ctx, "nixos.install", trace.WithAttributes(
		attribute.String("net.peer.ip", host),
		attribute.String("nix.flake_ref", flakeRef),
		attribute.String("ssh.target", target),
	))
	defer func() {
		if err != nil {
			span.SetStatus(codes.Error, err.Error())
			span.RecordError(err)
		}
		span.End()
	}()

	if sshKey == "" {
		return fmt.Errorf("install SSH key is required")
	}

	args := []string{
		"--flake", flakeRef,
		"--target-host", target,
		"--ssh-option", "ConnectTimeout=10",
		"--ssh-option", "ServerAliveInterval=5",
		"--ssh-option", "ServerAliveCountMax=3",
		"--ssh-option", "StrictHostKeyChecking=no",
		"--ssh-option", "UserKnownHostsFile=/dev/null",
		// In the case of PXE boot, where target machines are usually on the same LAN as the one running Nixie,
		// pushing from the Nix store where Nixie is running is usually faster than pulling from a remote cache over the internet.
		// Additionally, it's air-gapped.
		"--no-substitute-on-destination",
		"-i", sshKey,
	}

	cmd := exec.CommandContext(ctx, "nixos-anywhere", args...)

	if debug {
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
	}

	if err := cmd.Run(); err != nil {
		return err
	}

	return nil
}
