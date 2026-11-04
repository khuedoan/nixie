package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/http"
	"sync"
	"time"

	"code.khuedoan.com/nixie/internal/hosts"
	"code.khuedoan.com/nixie/internal/nixos"
	"code.khuedoan.com/nixie/internal/protocol"
	"code.khuedoan.com/nixie/internal/telemetry"

	"github.com/charmbracelet/log"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

type API struct {
	ctx             context.Context
	hostsConfig     hosts.HostsConfig
	sourceArchive   string
	readFingerprint func(context.Context, string) (string, error)
	results         chan<- error
	workers         sync.WaitGroup
}

func (api *API) status(w http.ResponseWriter, r *http.Request) {
	defer r.Body.Close()

	var result protocol.InstallResult
	if err := json.NewDecoder(r.Body).Decode(&result); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}

	name, err := hosts.GetFlakeOutputByMAC(result.MACAddress, api.hostsConfig)
	if err != nil {
		http.Error(w, err.Error(), http.StatusNotFound)
		return
	}

	host := api.hostsConfig[name]

	if result.Status == protocol.StatusStarted {
		host.CompareAndSwapState(hosts.StateUnknown, hosts.StateInstalling)
		state := host.GetState()
		if state != hosts.StateInstalling {
			http.Error(w, "host is not awaiting installation", http.StatusConflict)
			return
		}

		w.WriteHeader(http.StatusAccepted)
		return
	}

	if host.GetState() == hosts.StateUnknown {
		http.Error(w, "host has not started installation", http.StatusConflict)
		return
	}

	switch result.Status {
	case protocol.StatusProgress:
		if err := api.recordPhase(name, result); err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}

	case protocol.StatusFailed:
		api.failHost(host, name, errors.New(result.Error))

	case protocol.StatusCompleted:
		if host.CompareAndSwapState(hosts.StateInstalling, hosts.StateVerifying) {
			api.workers.Add(1)

			go func() {
				defer api.workers.Done()
				api.verifyHost(api.ctx, host, name)
			}()
		}

	default:
		http.Error(w, "unknown installation status", http.StatusBadRequest)
		return
	}

	w.WriteHeader(http.StatusAccepted)
}

func (api *API) recordPhase(name string, result protocol.InstallResult) error {
	switch result.Phase {
	case protocol.PhaseBoot,
		protocol.PhaseBuildDisko,
		protocol.PhasePartition,
		protocol.PhaseInstallSystem,
		protocol.PhaseReboot:
	default:
		return errors.New("unknown installation phase")
	}

	if result.Duration < 0 {
		return errors.New("phase duration must be nonnegative")
	}

	finishedAt := time.Now()
	startedAt := finishedAt.Add(-result.Duration)
	_, span := otel.Tracer("nixie").Start(api.ctx, "agent."+string(result.Phase),
		trace.WithTimestamp(startedAt),
		trace.WithAttributes(
			attribute.String("host.mac", result.MACAddress),
			attribute.String("host.name", name),
		),
	)

	if result.Error != "" {
		span.RecordError(errors.New(result.Error), trace.WithTimestamp(finishedAt))
		span.SetStatus(codes.Error, result.Error)
	}

	span.End(trace.WithTimestamp(finishedAt))
	return nil
}

func (api *API) verifyHost(ctx context.Context, host *hosts.Host, name string) {
	ctx, span := otel.Tracer("nixie").Start(ctx, "api.verify_host",
		trace.WithAttributes(
			attribute.String("host.mac", host.MACAddress),
			attribute.String("host.name", name),
		),
	)
	var verifyErr error
	defer func() { telemetry.End(span, verifyErr) }()

	fingerprint, verifyErr := api.readFingerprint(ctx, host.IP)
	if verifyErr != nil {
		api.failHost(host, name, verifyErr)

		return
	}

	if !host.CompareAndSwapState(hosts.StateVerifying, hosts.StateInstalled) {
		return
	}

	log.Info("successfully installed NixOS",
		"host", name,
		"ip", host.IP,
		"ssh_fingerprint", fingerprint,
	)

	api.results <- nil
}

func (api *API) failHost(host *hosts.Host, name string, err error) {
	if !host.Fail() {
		return
	}

	log.Error("host installation failed", "host", name, "error", err)
	api.results <- fmt.Errorf("host %s: %w", name, err)
}

func (api *API) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("POST /status", api.status)
	mux.HandleFunc("GET /source.tar.gz", func(w http.ResponseWriter, r *http.Request) {
		http.ServeFile(w, r, api.sourceArchive)
	})
	return mux
}

func StartAPIServer(
	ctx context.Context,
	hostsConfig hosts.HostsConfig,
	address, sourceArchive string,
	results chan<- error,
) error {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()

	api := &API{
		ctx:             ctx,
		hostsConfig:     hostsConfig,
		sourceArchive:   sourceArchive,
		readFingerprint: nixos.ReadHostFingerprint,
		results:         results,
	}

	server := &http.Server{
		Addr:    net.JoinHostPort(address, "5000"),
		Handler: api.handler(),
	}

	shutdownDone := make(chan struct{})
	go func() {
		defer close(shutdownDone)
		<-ctx.Done()
		shutdownCtx, stop := context.WithTimeout(context.Background(), 5*time.Second)
		defer stop()

		if err := server.Shutdown(shutdownCtx); err != nil {
			server.Close()
		}
	}()

	log.Info("starting API server", "address", server.Addr)
	err := server.ListenAndServe()

	cancel()
	<-shutdownDone
	api.workers.Wait()

	if !errors.Is(err, http.ErrServerClosed) {
		return err
	}

	return nil
}
