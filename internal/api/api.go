package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"sync"

	"code.khuedoan.com/nixie/internal/hosts"
	"code.khuedoan.com/nixie/internal/nixos"
	"github.com/charmbracelet/log"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

type API struct {
	ctx              context.Context
	hostsConfig      hosts.HostsConfig
	hostsFile        string
	saveMu           sync.Mutex
	flake            string
	installSSHKey    string
	deploymentSSHKey string
	debug            bool
	doneCh           chan struct{}
}

type InstallRequest struct {
	MACAddress string `json:"mac_address"`
}

func (api *API) ping(w http.ResponseWriter, r *http.Request) {
	ip := extractClientIP(r)
	log.Info("received ping from agent", "ip", ip)
	io.WriteString(w, "pong")
}

func (api *API) install(w http.ResponseWriter, r *http.Request) {
	var installRequest InstallRequest
	if err := json.NewDecoder(r.Body).Decode(&installRequest); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	defer r.Body.Close()

	ip := extractClientIP(r)
	ctx, span := otel.Tracer("nixie").Start(api.ctx, "api.install_request", trace.WithAttributes(
		attribute.String("net.peer.ip", ip),
		attribute.String("host.mac", installRequest.MACAddress),
	))
	defer span.End()

	log.Info("received install request from agent", "ip", ip, "request", installRequest)
	flakeOutput, err := hosts.GetFlakeOutputByMAC(installRequest.MACAddress, api.hostsConfig)
	if err != nil {
		span.SetStatus(codes.Error, err.Error())
		span.RecordError(err)
		log.Error("failed to get flake by MAC address", "err", err)
		http.Error(w, err.Error(), http.StatusNotFound)
		return
	}
	flake := fmt.Sprintf("%s#%s", api.flake, flakeOutput)
	host := api.hostsConfig[flakeOutput]

	// TODO need better condition here
	if host.GetState() != hosts.StateUnknown {
		err := fmt.Errorf("installation already in progress")
		span.SetStatus(codes.Error, err.Error())
		span.RecordError(err)
		http.Error(w, err.Error(), http.StatusConflict)
		return
	}
	host.SetState(hosts.StateInstalling)

	log.Info("installing NixOS", "host", ip, "flake", flake)
	go func() {
		if err := api.installHost(ctx, host, flakeOutput, flake, ip); err != nil {
			log.Error("failed to install host", "ip", ip, "flake", flake, "error", err)
			host.SetState(hosts.StateFailed)
		}

		if hosts.AllInstalled(api.hostsConfig) {
			log.Debug("all hosts installed, signaling completion")
			select {
			case api.doneCh <- struct{}{}:
				log.Debug("completion signal sent", "channel", "doneCh")
			default:
				log.Debug("completion already signaled, skipping")
			}
		}
	}()

	w.WriteHeader(http.StatusAccepted)
	io.WriteString(w, "installation started")
}

func (api *API) installHost(ctx context.Context, host *hosts.Host, flakeOutput, flake, ip string) (err error) {
	ctx, span := otel.Tracer("nixie").Start(ctx, "api.install_host", trace.WithAttributes(
		attribute.String("host.mac", host.MACAddress.String()),
		attribute.String("net.peer.ip", ip),
		attribute.String("nix.flake_output", flakeOutput),
		attribute.String("nix.flake_ref", flake),
	))
	defer func() {
		if err != nil {
			span.SetStatus(codes.Error, err.Error())
			span.RecordError(err)
		}
		span.End()
	}()

	if err := nixos.Install(ctx, flake, "root", ip, api.installSSHKey, api.debug); err != nil {
		return fmt.Errorf("failed to install NixOS: %w", err)
	}

	machineIDHash, err := nixos.ReadMachineIDHash(ctx, "root", ip, api.deploymentSSHKey, api.debug)
	if err != nil {
		return fmt.Errorf("failed to read final machine ID: %w", err)
	}
	span.SetAttributes(attribute.String("host.machine_id_hash", machineIDHash))
	host.SetFinalIdentity(ip, machineIDHash)
	if err := api.saveHosts(); err != nil {
		return fmt.Errorf("failed to save hosts config %s: %w", api.hostsFile, err)
	}
	log.Info("successfully installed NixOS", "ip", ip, "flake", flake)
	host.SetState(hosts.StateInstalled)
	return nil
}

func (api *API) saveHosts() error {
	api.saveMu.Lock()
	defer api.saveMu.Unlock()

	return hosts.SaveHostsConfig(api.hostsFile, api.hostsConfig)
}

func StartAPIServer(ctx context.Context, hostsConfig hosts.HostsConfig, hostsFile string, flake string, installSSHKey string, deploymentSSHKey string, debug bool, doneCh chan struct{}) error {
	api := &API{
		ctx:              ctx,
		hostsConfig:      hostsConfig,
		hostsFile:        hostsFile,
		flake:            flake,
		installSSHKey:    installSSHKey,
		deploymentSSHKey: deploymentSSHKey,
		debug:            debug,
		doneCh:           doneCh,
	}
	mux := http.NewServeMux()
	mux.HandleFunc("GET /ping", api.ping)
	mux.HandleFunc("POST /install", api.install)

	server := &http.Server{
		Addr:    ":5000",
		Handler: mux,
	}
	log.Info("starting API server", "address", server.Addr)

	go func() {
		<-ctx.Done()
		server.Shutdown(ctx)
	}()

	if err := server.ListenAndServe(); !errors.Is(err, http.ErrServerClosed) {
		return err
	}
	return nil
}

func extractClientIP(r *http.Request) string {
	ip, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}

	return ip
}
