package nixos

import (
	"context"
	"fmt"
	"os"
	"os/exec"
)

func Install(ctx context.Context, flakeRef, user, host, password string, debug bool) error {
	cmd := exec.CommandContext(
		ctx,
		"nixos-anywhere",
		"--flake", flakeRef,
		"--target-host", fmt.Sprintf("%s@%s", user, host),
		"--env-password",
		// In the case of PXE boot, where target machines are usually on the same LAN as the one running Nixie,
		// pushing from the Nix store where Nixie is running is usually faster than pulling from a remote cache over the internet.
		// Additionally, it's air-gapped.
		"--no-substitute-on-destination",
	)

	cmd.Env = append(os.Environ(), fmt.Sprintf("SSHPASS=%s", password))

	if debug {
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
	}

	return cmd.Run()
}
