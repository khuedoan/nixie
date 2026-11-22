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

	bundle, err := nixBuild(ctx, flakeRef+".config.system.build.nixieInstaller", debug)
	if err != nil {
		span.SetStatus(codes.Error, err.Error())
		span.RecordError(err)
		return InstallerComponents{}, err
	}
	initTarget, err := os.Readlink(filepath.Join(bundle, "init"))
	if err != nil {
		err = fmt.Errorf("read installer init path: %w", err)
		span.SetStatus(codes.Error, err.Error())
		span.RecordError(err)
		return InstallerComponents{}, err
	}
	return InstallerComponents{
		Kernel: filepath.Join(bundle, "kernel"),
		Initrd: filepath.Join(bundle, "initrd"),
		Init:   initTarget,
	}, nil
}
