package nixos

import (
	"bytes"
	"context"
	"encoding/hex"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

const (
	machineIDReadTimeout  = 10 * time.Minute
	machineIDReadInterval = 2 * time.Second
)

func VerifyInstalledHost(ctx context.Context, user, host, sshKey, sshAgentSocket string, debug bool) (err error) {
	ctx, span := otel.Tracer("nixie").Start(ctx, "nixos.verify_installed_host", trace.WithAttributes(
		attribute.String("net.peer.ip", host),
		attribute.String("ssh.target", sshTarget(user, host)),
	))
	defer func() {
		if err != nil {
			span.SetStatus(codes.Error, err.Error())
			span.RecordError(err)
		}
		span.End()
	}()

	if sshKey == "" && sshAgentSocket == "" {
		return fmt.Errorf("deployment SSH key or SSH agent socket is required")
	}

	attempts := 0
	timeout := time.After(machineIDReadTimeout)

	for {
		attempts++
		err = verifyInstalledHostOnce(ctx, user, host, sshKey, sshAgentSocket, debug)
		if err == nil {
			span.SetAttributes(attribute.Int("ssh.attempts", attempts))
			return nil
		}
		span.SetAttributes(
			attribute.Int("ssh.attempts", attempts),
			attribute.String("ssh.last_error", err.Error()),
		)

		select {
		case <-ctx.Done():
			return fmt.Errorf("read final machine ID: %w", ctx.Err())
		case <-timeout:
			return fmt.Errorf("timed out reading final machine ID from %s: %w", host, err)
		case <-time.After(machineIDReadInterval):
		}
	}
}

func verifyInstalledHostOnce(ctx context.Context, user, host, sshKey, sshAgentSocket string, debug bool) error {
	args := []string{
		"-o", "BatchMode=yes",
		"-o", "ConnectTimeout=5",
		"-o", "StrictHostKeyChecking=no",
		"-o", "UserKnownHostsFile=/dev/null",
	}
	if sshKey != "" {
		args = append(args, "-i", sshKey)
	}
	args = append(args, sshTarget(user, host), "cat /etc/machine-id")

	cmd := exec.CommandContext(ctx, "ssh", args...)
	cmd.Env = sshEnv(sshAgentSocket)
	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	if debug {
		cmd.Stderr = os.Stderr
	} else {
		cmd.Stderr = &stderr
	}

	if err := cmd.Run(); err != nil {
		message := strings.TrimSpace(stderr.String())
		if message != "" {
			return fmt.Errorf("ssh failed: %w: %s", err, message)
		}
		return fmt.Errorf("ssh failed: %w", err)
	}

	machineID := strings.TrimSpace(stdout.String())
	if len(machineID) != 32 {
		return fmt.Errorf("invalid machine ID length: got %d, want 32", len(machineID))
	}
	if _, err := hex.DecodeString(machineID); err != nil {
		return fmt.Errorf("machine ID is not hexadecimal")
	}
	return nil
}

func sshTarget(user, host string) string {
	if strings.Contains(host, ":") && !strings.HasPrefix(host, "[") {
		host = "[" + host + "]"
	}
	return fmt.Sprintf("%s@%s", user, host)
}

func sshEnv(sshAgentSocket string) []string {
	env := os.Environ()
	if sshAgentSocket != "" {
		env = append(env, "SSH_AUTH_SOCK="+sshAgentSocket)
	}
	return env
}
