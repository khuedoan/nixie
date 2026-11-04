package nixos

import (
	"context"
	"errors"
	"fmt"
	"net"
	"time"

	"golang.org/x/crypto/ssh"
)

var errFingerprintObserved = errors.New("SSH host fingerprint observed")

// ReadHostFingerprint returns the SHA256 fingerprint of the Ed25519 host key on port 22.
// It verifies key possession without authentication and retries for up to ten minutes, bounded by ctx.
func ReadHostFingerprint(ctx context.Context, host string) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, 10*time.Minute)
	defer cancel()

	for {
		fingerprint, err := readHostFingerprintOnce(ctx, net.JoinHostPort(host, "22"))
		if err == nil {
			return fingerprint, nil
		}

		select {
		case <-ctx.Done():
			return "", fmt.Errorf("read SSH host fingerprint from %s: %w", host, errors.Join(ctx.Err(), err))
		case <-time.After(2 * time.Second):
		}
	}
}

func readHostFingerprintOnce(ctx context.Context, address string) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()

	connection, err := (&net.Dialer{}).DialContext(ctx, "tcp", address)
	if err != nil {
		return "", err
	}

	defer connection.Close()

	deadline, _ := ctx.Deadline()
	if err := connection.SetDeadline(deadline); err != nil {
		return "", err
	}

	stop := context.AfterFunc(ctx, func() { connection.Close() })
	defer stop()

	var fingerprint string
	config := &ssh.ClientConfig{
		HostKeyAlgorithms: []string{ssh.KeyAlgoED25519},
		HostKeyCallback: func(_ string, _ net.Addr, key ssh.PublicKey) error {
			fingerprint = ssh.FingerprintSHA256(key)
			return errFingerprintObserved
		},
	}

	_, _, _, err = ssh.NewClientConn(connection, address, config)

	if ctx.Err() != nil {
		return "", ctx.Err()
	}

	// The SSH library verifies the exchange signature before invoking HostKeyCallback.
	if errors.Is(err, errFingerprintObserved) {
		return fingerprint, nil
	}

	return "", fmt.Errorf("probe SSH host key: %w", err)
}
