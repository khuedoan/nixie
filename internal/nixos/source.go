package nixos

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
)

// ArchiveFlake archives Nix's source snapshot and returns the flake's directory within it.
// Relative inputs must remain within the source tree. Git sources include tracked files only.
func ArchiveFlake(ctx context.Context, ref, destination string) (string, error) {
	cmd := exec.CommandContext(ctx, "nix", "flake", "metadata",
		"--extra-experimental-features", "nix-command flakes",
		"--json",
		"--no-write-lock-file",
		ref,
	)
	cmd.Stderr = os.Stderr
	metadata, err := cmd.Output()
	if err != nil {
		return "", fmt.Errorf("snapshot flake source: %w", err)
	}

	var source struct {
		Path     string
		Resolved struct{ Dir string }
	}

	if err := json.Unmarshal(metadata, &source); err != nil {
		return "", fmt.Errorf("decode flake source metadata: %w", err)
	}

	if !filepath.IsAbs(source.Path) {
		return "", fmt.Errorf("flake source metadata has no absolute snapshot path")
	}

	archive := exec.CommandContext(ctx, "tar",
		"--create",
		"--gzip",
		"--file", destination,
		"--directory", filepath.Dir(source.Path),
		"--", filepath.Base(source.Path),
	)
	archive.Stderr = os.Stderr

	if err := archive.Run(); err != nil {
		os.Remove(destination)
		return "", fmt.Errorf("archive flake source: %w", err)
	}

	return source.Resolved.Dir, nil
}
