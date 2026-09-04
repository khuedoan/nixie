package nixos

import (
	"fmt"
	"os"
	"path/filepath"
)

type askpass struct {
	env     []string
	cleanup func()
}

func newAskpass(passphraseFile string) (*askpass, error) {
	if passphraseFile == "" {
		return &askpass{
			env:     os.Environ(),
			cleanup: func() {},
		}, nil
	}

	passphrase, err := os.ReadFile(passphraseFile)
	if err != nil {
		return nil, fmt.Errorf("read SSH key passphrase file: %w", err)
	}
	if len(passphrase) == 0 {
		return nil, fmt.Errorf("SSH key passphrase file is empty")
	}

	dir, err := os.MkdirTemp("", "nixie-askpass-*")
	if err != nil {
		return nil, fmt.Errorf("create askpass temp dir: %w", err)
	}

	cleanup := func() {
		os.RemoveAll(dir)
	}

	passphrasePath := filepath.Join(dir, "passphrase")
	if err := os.WriteFile(passphrasePath, passphrase, 0o600); err != nil {
		cleanup()
		return nil, fmt.Errorf("write askpass passphrase file: %w", err)
	}

	scriptPath := filepath.Join(dir, "ssh-askpass")
	script := "#!/bin/sh\ncat \"$0.passphrase\"\n"
	if err := os.WriteFile(scriptPath, []byte(script), 0o700); err != nil {
		cleanup()
		return nil, fmt.Errorf("write askpass helper: %w", err)
	}
	if err := os.Rename(passphrasePath, scriptPath+".passphrase"); err != nil {
		cleanup()
		return nil, fmt.Errorf("prepare askpass passphrase file: %w", err)
	}

	env := append(os.Environ(),
		"SSH_ASKPASS="+scriptPath,
		"SSH_ASKPASS_REQUIRE=force",
		"DISPLAY=nixie",
	)

	return &askpass{
		env:     env,
		cleanup: cleanup,
	}, nil
}
