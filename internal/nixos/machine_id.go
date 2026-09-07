package nixos

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"

	"code.khuedoan.com/nixie/internal/hosts"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

const (
	machineIDReadTimeout  = 10 * time.Minute
	machineIDReadInterval = 2 * time.Second
)

func ReadMachineIDHash(ctx context.Context, user, host, sshKey, sshAgentSocket string, debug bool) (machineIDHash string, err error) {
	ctx, span := otel.Tracer("nixie").Start(ctx, "nixos.read_machine_id_hash", trace.WithAttributes(
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

	if sshKey == "" {
		return "", fmt.Errorf("deployment SSH key is required")
	}

	var lastErr error
	attempts := 0
	timeout := time.After(machineIDReadTimeout)

	for {
		attempts++
		machineIDHash, err = readMachineIDHashOnce(ctx, user, host, sshKey, sshAgentSocket, debug)
		if err == nil {
			span.SetAttributes(
				attribute.Int("ssh.attempts", attempts),
				attribute.String("host.machine_id_hash", machineIDHash),
			)
			return machineIDHash, nil
		}
		lastErr = err
		span.SetAttributes(
			attribute.Int("ssh.attempts", attempts),
			attribute.String("ssh.last_error", err.Error()),
		)

		select {
		case <-ctx.Done():
			return "", fmt.Errorf("read final machine ID: %w", ctx.Err())
		case <-timeout:
			return "", fmt.Errorf("timed out reading final machine ID from %s: %w", host, lastErr)
		case <-time.After(machineIDReadInterval):
		}
	}
}

func readMachineIDHashOnce(ctx context.Context, user, host, sshKey, sshAgentSocket string, debug bool) (string, error) {
	args := []string{
		"-o", "BatchMode=yes",
		"-o", "ConnectTimeout=5",
		"-o", "StrictHostKeyChecking=no",
		"-o", "UserKnownHostsFile=/dev/null",
		"-i", sshKey,
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
			return "", fmt.Errorf("ssh failed: %w: %s", err, message)
		}
		return "", fmt.Errorf("ssh failed: %w", err)
	}

	machineIDHash, err := hosts.HashMachineID(stdout.String())
	if err != nil {
		return "", fmt.Errorf("failed to hash machine ID: %w", err)
	}

	return machineIDHash, nil
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
