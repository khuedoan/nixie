package main

import (
	"context"
	"errors"
	"testing"
)

func TestWaitForHostsCollectsAllFailures(t *testing.T) {
	first := errors.New("first host failed")
	second := errors.New("second host failed")
	results := make(chan error)
	done := make(chan error, 1)
	go func() {
		done <- waitForHosts(t.Context(), 2, results, nil)
	}()

	results <- first

	select {
	case results <- second:
	case err := <-done:
		t.Fatalf("stopped before the second host finished: %v", err)
	}

	err := <-done
	if !errors.Is(err, first) || !errors.Is(err, second) {
		t.Fatalf("result = %v, want both host errors", err)
	}
}

func TestWaitForHostsReturnsCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	cancel()

	if err := waitForHosts(ctx, 1, nil, nil); !errors.Is(err, context.Canceled) {
		t.Fatalf("result = %v, want cancellation", err)
	}
}

func TestWaitForHostsPreservesServerError(t *testing.T) {
	failure := errors.New("API address already in use")
	serverErrors := make(chan error, 1)
	serverErrors <- failure

	if err := waitForHosts(t.Context(), 1, nil, serverErrors); !errors.Is(err, failure) {
		t.Fatalf("result = %v, want server error", err)
	}
}
