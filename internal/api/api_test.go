package api

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"code.khuedoan.com/nixie/internal/hosts"
	"code.khuedoan.com/nixie/internal/protocol"
)

const nodeMAC = "02:00:00:00:00:01"

func testAPI(t *testing.T) (*API, <-chan error) {
	t.Helper()
	results := make(chan error, 2)
	api := &API{
		ctx: t.Context(),
		hostsConfig: hosts.HostsConfig{
			"node": {
				MACAddress: nodeMAC,
				IP:         "192.0.2.10",
			},
		},
		results: results,
	}
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

func TestAdmissionAndCompletion(t *testing.T) {
	api, results := testAPI(t)
	const otherMAC = "02:00:00:00:00:02"
	api.hostsConfig["other"] = &hosts.Host{MACAddress: otherMAC, IP: "192.0.2.11"}
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
		go func() {
			postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusCompleted}, http.StatusAccepted)
			finished <- struct{}{}
		}()
	}

	for range 2 {
		<-finished
	}

	postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusStarted}, http.StatusConflict)

	if api.hostsConfig["node"].GetState() != hosts.StateInstalled {
		t.Fatal("completed host was not marked installed")
	}

	if err := <-results; err != nil {
		t.Fatalf("first host failed: %v", err)
	}
	assertNoResult(t, results)
	postStatus(t, api, protocol.InstallResult{MACAddress: otherMAC, Status: protocol.StatusCompleted}, http.StatusAccepted)

	select {
	case err := <-results:
		if err != nil {
			t.Fatalf("second host failed: %v", err)
		}
	default:
		t.Fatal("completed host did not report a result")
	}

	for _, result := range []protocol.InstallResult{
		{MACAddress: nodeMAC, Status: protocol.StatusCompleted},
		{MACAddress: nodeMAC, Status: protocol.StatusFailed, Error: "late duplicate"},
	} {
		postStatus(t, api, result, http.StatusAccepted)
	}

	postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusStarted}, http.StatusConflict)

	if api.hostsConfig["node"].GetState() != hosts.StateInstalled {
		t.Fatal("duplicate terminal result changed the installed host")
	}

	assertNoResult(t, results)
}

func TestFailureReportsOnce(t *testing.T) {
	api, results := testAPI(t)
	postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusStarted}, http.StatusAccepted)

	for range 2 {
		postStatus(t, api, protocol.InstallResult{
			MACAddress: nodeMAC,
			Error:      "schedule reboot: local operation failed",
			Status:     protocol.StatusFailed,
		}, http.StatusAccepted)
	}

	if api.hostsConfig["node"].GetState() != hosts.StateFailed {
		t.Fatal("installation failure was not retained")
	}

	select {
	case err := <-results:
		if err == nil || !strings.Contains(err.Error(), "host node: schedule reboot:") {
			t.Fatalf("failure result = %v, want host error", err)
		}
	default:
		t.Fatal("failed host did not report a result")
	}

	postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusCompleted}, http.StatusAccepted)
	if api.hostsConfig["node"].GetState() != hosts.StateFailed {
		t.Fatal("late completion changed the failed host")
	}

	assertNoResult(t, results)
	postStatus(t, api, protocol.InstallResult{MACAddress: nodeMAC, Status: protocol.StatusStarted}, http.StatusConflict)
}
