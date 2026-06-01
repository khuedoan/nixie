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

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

type InstallerComponents struct {
	Kernel string
	Initrd string
	Init   string
}

func nixBuild(ctx context.Context, flakeOutput string, debug bool) (string, error) {
	_, span := otel.Tracer("nixie").Start(ctx, "nix.build", trace.WithAttributes(attribute.String("nix.flake_output", flakeOutput)))
	defer span.End()

	cmd := exec.CommandContext(ctx, "nix", "build", "--no-link", "--print-out-paths", flakeOutput)

	var stdout bytes.Buffer
	stdoutWriters := []io.Writer{&stdout}
	if debug {
		stdoutWriters = append(stdoutWriters, os.Stdout)
	}

	cmd.Stdout = io.MultiWriter(stdoutWriters...)
	cmd.Stderr = os.Stderr

	if err := cmd.Run(); err != nil {
		err = fmt.Errorf("nix build failed for %q: %w", flakeOutput, err)
		span.SetStatus(codes.Error, err.Error())
		span.RecordError(err)
		return "", err
	}

	outPath := strings.TrimSpace(stdout.String())
	return outPath, nil
}

func BuildInstaller(ctx context.Context, flakeRef string, debug bool) (InstallerComponents, error) {
	ctx, span := otel.Tracer("nixie").Start(ctx, "nixie.build_installer", trace.WithAttributes(attribute.String("nixie.installer", flakeRef)))
	defer span.End()

	// TODO there might be some race condition here if we update the flake/installer content while an existing build is running, causing mismatch in init path and the actual one in the installer
	kernelOut, err := nixBuild(
		ctx,
		flakeRef+".config.system.build.kernel",
		debug,
	)
	if err != nil {
		err = fmt.Errorf("failed to build kernel: %w", err)
		span.SetStatus(codes.Error, err.Error())
		span.RecordError(err)
		return InstallerComponents{}, err
	}

	initrdOut, err := nixBuild(
		ctx,
		flakeRef+".config.system.build.netbootRamdisk",
		debug,
	)

	if err != nil {
		err = fmt.Errorf("failed to build initrd: %w", err)
		span.SetStatus(codes.Error, err.Error())
		span.RecordError(err)
		return InstallerComponents{}, err
	}
	toplevelOut, err := nixBuild(
		ctx,
		flakeRef+".config.system.build.toplevel",
		debug,
	)

	if err != nil {
		err = fmt.Errorf("failed to build toplevel: %w", err)
		span.SetStatus(codes.Error, err.Error())
		span.RecordError(err)
		return InstallerComponents{}, err
	}

	components := InstallerComponents{
		Kernel: filepath.Join(kernelOut, "bzImage"),
		Initrd: filepath.Join(initrdOut, "initrd"),
		Init:   filepath.Join(toplevelOut, "init"),
	}

	return components, nil
}
