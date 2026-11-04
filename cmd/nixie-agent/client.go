package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"time"

	"code.khuedoan.com/nixie/internal/protocol"
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
