package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/http"
	"time"

	"code.khuedoan.com/nixie/internal/protocol"

	"golang.org/x/sys/unix"
)

type statusReporter struct {
	address string
	mac     string
}

func install(ctx context.Context, config AgentConfig) error {
	reporter := statusReporter{
		address: "http://" + config.APIAddress,
		mac:     config.MACAddress,
	}

	if err := installFlake(ctx, config.Flake, reporter); err != nil {
		reportCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), 10*time.Second)
		defer cancel()

		reportErr := reporter.send(reportCtx, protocol.InstallResult{
			Status: protocol.StatusFailed,
			Error:  err.Error(),
		})
		return errors.Join(err, reportErr)
	}

	return nil
}

func (reporter statusReporter) send(ctx context.Context, result protocol.InstallResult) error {
	result.MACAddress = reporter.mac
	body, err := json.Marshal(result)
	if err != nil {
		return err
	}

	client := &http.Client{Timeout: 10 * time.Second}
	var lastErr error

	for attempt := 0; attempt < 3; attempt++ {
		request, err := http.NewRequestWithContext(
			ctx,
			http.MethodPost,
			reporter.address+"/status",
			bytes.NewReader(body),
		)
		if err != nil {
			return err
		}

		request.Header.Set("Content-Type", "application/json")
		response, err := client.Do(request)
		if err == nil {
			response.Body.Close()

			if response.StatusCode == http.StatusAccepted {
				return nil
			}

			err = fmt.Errorf("report installation result: %s", response.Status)
		}

		lastErr = err

		if attempt < 2 {
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-time.After(time.Second):
			}
		}
	}

	return lastErr
}

// reportBoot measures kernel startup through agent admission, at second resolution.
func (reporter statusReporter) reportBoot(ctx context.Context) {
	var info unix.Sysinfo_t
	if err := unix.Sysinfo(&info); err != nil {
		log.Printf("read installer uptime: %v", err)
		return
	}

	started := time.Now().Add(-time.Duration(info.Uptime) * time.Second)
	reporter.reportPhase(ctx, protocol.PhaseBoot, started, nil)
}

func (reporter statusReporter) reportPhase(ctx context.Context, phase protocol.InstallPhase, started time.Time, phaseErr error) {
	result := protocol.InstallResult{
		Status:   protocol.StatusProgress,
		Phase:    phase,
		Duration: time.Since(started),
	}

	if phaseErr != nil {
		result.Error = phaseErr.Error()
	}

	reportCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), 2*time.Second)
	defer cancel()

	if err := reporter.send(reportCtx, result); err != nil {
		log.Printf("report completed phase %s: %v", phase, err)
	}
}
