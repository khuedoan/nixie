package nixos

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"code.khuedoan.com/nixie/internal/telemetry"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/trace"
)

type InstallerComponents struct {
	Kernel string
	Initrd string
	Init   string
}

func BuildInstaller(ctx context.Context, flakeRef string, debug bool) (components InstallerComponents, err error) {
	flakeOutput := flakeRef + ".config.system.build.nixieInstaller"
	_, span := otel.Tracer("nixie").Start(ctx, "nixie.build_installer",
		trace.WithAttributes(attribute.String("nix.flake_output", flakeOutput)),
	)
	defer func() { telemetry.End(span, err) }()

	cmd := exec.CommandContext(ctx, "nix", "build",
		"--extra-experimental-features", "nix-command flakes",
		"--no-write-lock-file",
		"--no-link",
		"--print-out-paths",
		flakeOutput,
	)

	var stdout bytes.Buffer
	stdoutWriters := []io.Writer{&stdout}

	if debug {
		stdoutWriters = append(stdoutWriters, os.Stdout)
	}

	cmd.Stdout = io.MultiWriter(stdoutWriters...)
	cmd.Stderr = os.Stderr

	if err = cmd.Run(); err != nil {
		err = fmt.Errorf("nix build failed for %q: %w", flakeOutput, err)
		return InstallerComponents{}, err
	}

	bundle := strings.TrimSpace(stdout.String())

	// The bundle itself is not in the netboot image; boot the original NixOS init path.
	init, err := os.Readlink(filepath.Join(bundle, "init"))
	if err != nil {
		return InstallerComponents{}, fmt.Errorf("read installer init path: %w", err)
	}

	return InstallerComponents{
		Kernel: filepath.Join(bundle, "kernel"),
		Initrd: filepath.Join(bundle, "initrd"),
		Init:   init,
	}, nil
}
