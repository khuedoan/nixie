package api

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"code.khuedoan.com/nixie/internal/hosts"
	"code.khuedoan.com/nixie/internal/protocol"
)

const nodeMAC = "02:00:00:00:00:01"
const testFingerprint = "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"

func testAPI(t *testing.T) (*API, <-chan error) {
	t.Helper()
	ctx, cancel := context.WithCancel(t.Context())
	results := make(chan error, 2)
	api := &API{
		ctx: ctx,
		hostsConfig: hosts.HostsConfig{
			"node": {
				MACAddress: nodeMAC,
				IP:         "192.0.2.10",
			},
		},
		readFingerprint: func(context.Context, string) (string, error) { return testFingerprint, nil },
		results:         results,
	}
	t.Cleanup(func() {
		cancel()
		api.workers.Wait()
	})
	return api, results
}

func postStatus(t *testing.T, api *API, result protocol.InstallResult, want int) {
	t.Helper()
	body, _ := json.Marshal(result)
	request := httptest.NewRequest(http.MethodPost, "/status", bytes.NewReader(body))
	response := httptest.NewRecorder()
	api.handler().ServeHTTP(response, request)

	if response.Code != want {
		t.Errorf("status %+v returned %d: %s", result, response.Code, response.Body.String())
	}
}

func assertNoResult(t *testing.T, results <-chan error) {
	t.Helper()

	select {
	case <-results:
		t.Fatal("received an unexpected terminal result")
	default:
	}
}

func TestAdmissionVerificationAndCompletion(t *testing.T) {
	api, results := testAPI(t)
	const otherMAC = "02:00:00:00:00:02"
	api.hostsConfig["other"] = &hosts.Host{MACAddress: otherMAC, IP: "192.0.2.11"}
	release, addresses := make(chan struct{}), make(chan string, 4)
	var probes atomic.Int32
	api.readFingerprint = func(ctx context.Context, address string) (string, error) {
		probes.Add(1)
		addresses <- address

		select {
		case <-release:
			return testFingerprint, nil
		case <-ctx.Done():
			return "", ctx.Err()
		}
	}
	postStatus(t, api, protocol.InstallResult{MACAddress: "02:00:00:00:00:ff", Status: protocol.StatusStarted}, http.StatusNotFound)
	postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusCompleted}, http.StatusConflict)
	finished := make(chan struct{}, 4)

	for _, mac := range []string{nodeMAC, otherMAC, nodeMAC, otherMAC} {
		go func() {
			postStatus(t, api, protocol.InstallResult{MACAddress: mac, Status: protocol.StatusStarted}, http.StatusAccepted)
			finished <- struct{}{}
		}()
	}

	for range 4 {
		<-finished
	}

	postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC}, http.StatusBadRequest)
	postStatus(t, api, protocol.InstallResult{
		MACAddress: nodeMAC,
		Status:     protocol.StatusProgress,
		Phase:      protocol.PhasePartition,
		Duration:   time.Millisecond,
	}, http.StatusAccepted)

	for range 2 {
		postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusCompleted}, http.StatusAccepted)
	}

	postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusStarted}, http.StatusConflict)

	if api.hostsConfig["node"].GetState() != hosts.StateVerifying {
		t.Fatal("host was marked installed before verification")
	}

	close(release)
	api.workers.Wait()
	if err := <-results; err != nil {
		t.Fatalf("first host failed: %v", err)
	}
	assertNoResult(t, results)
	postStatus(t, api, protocol.InstallResult{MACAddress: otherMAC, Status: protocol.StatusCompleted}, http.StatusAccepted)
	api.workers.Wait()

	select {
	case err := <-results:
		if err != nil {
			t.Fatalf("second host failed: %v", err)
		}
	default:
		t.Fatal("verified host did not report a result")
	}

	if probes.Load() != 2 || <-addresses != "192.0.2.10" || <-addresses != "192.0.2.11" {
		t.Fatal("verification did not use each configured address exactly once")
	}

	for _, result := range []protocol.InstallResult{
		{MACAddress: nodeMAC, Status: protocol.StatusCompleted},
		{MACAddress: nodeMAC, Status: protocol.StatusFailed, Error: "late duplicate"},
	} {
		postStatus(t, api, result, http.StatusAccepted)
	}

	postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusStarted}, http.StatusConflict)
	api.workers.Wait()

	if probes.Load() != 2 || api.hostsConfig["node"].GetState() != hosts.StateInstalled {
		t.Fatal("duplicate terminal result changed the installed host")
	}

	assertNoResult(t, results)
}

func TestFailuresReportOnce(t *testing.T) {
	for _, failure := range []string{"agent", "reboot", "probe"} {
		t.Run(failure, func(t *testing.T) {
			api, results := testAPI(t)
			release := make(chan struct{})
			api.readFingerprint = func(ctx context.Context, _ string) (string, error) {
				if failure == "reboot" {
					select {
					case <-release:
					case <-ctx.Done():
						return "", ctx.Err()
					}
				}

				if failure == "probe" {
					return "", errors.New("unreachable")
				}

				return testFingerprint, nil
			}
			postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusStarted}, http.StatusAccepted)

			if failure != "agent" {
				postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusCompleted}, http.StatusAccepted)
			}

			if failure == "agent" || failure == "reboot" {
				for range 2 {
					postStatus(t, api, protocol.InstallResult{
						MACAddress: nodeMAC,
						Error:      "local operation failed",
						Status:     protocol.StatusFailed,
					}, http.StatusAccepted)
				}
			}

			close(release)
			api.workers.Wait()

			if api.hostsConfig["node"].GetState() != hosts.StateFailed {
				t.Fatal("installation failure was not retained")
			}

			select {
			case err := <-results:
				if err == nil || !strings.Contains(err.Error(), "host node") {
					t.Fatalf("failure result = %v, want host error", err)
				}
			default:
				t.Fatal("failed host did not report a result")
			}

			assertNoResult(t, results)
			postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusStarted}, http.StatusConflict)
		})
	}
}
