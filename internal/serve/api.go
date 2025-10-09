package serve

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"

	"code.khuedoan.com/nixie/internal/hosts"
	"code.khuedoan.com/nixie/internal/nixos"
	"github.com/charmbracelet/log"
)

type API struct {
	ctx         context.Context
	hostsConfig hosts.HostsConfig
	flake       string
	debug       bool
	doneCh      chan struct{}
}

type InstallRequest struct {
	MACAddress string
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
	log.Info("received install request from agent", "ip", ip, "request", installRequest)
	flakeOutput, err := hosts.GetFlakeOutputByMAC(installRequest.MACAddress, api.hostsConfig)
	if err != nil {
		log.Error("failed to get flake by MAC address", "err", err)
		http.Error(w, err.Error(), http.StatusNotFound)
		return
	}
	flake := fmt.Sprintf("%s#%s", api.flake, flakeOutput)
	host := api.hostsConfig[flakeOutput]

	// TODO need better condition here
	if host.GetState() != hosts.StateUnknown {
		http.Error(w, "installation already in progress", http.StatusConflict)
		return
	}
	host.SetState(hosts.StateInstalling)

	log.Info("installing NixOS", "host", ip, "flake", flake)
	go func() {
		// TODO IMPORTANT support SSH key
		if err := nixos.Install(api.ctx, flake, "root", ip, "nixos-installer", api.debug); err != nil {
			log.Error("failed to install NixOS", "ip", ip, "flake", flake, "error", err)
			host.SetState(hosts.StateFailed)
		} else {
			log.Info("successfully installed NixOS", "ip", ip, "flake", flake)
			host.SetState(hosts.StateInstalled)
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

func (api *API) router() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /ping", api.ping)
	mux.HandleFunc("POST /install", api.install)

	return mux
}

func StartAPIServer(ctx context.Context, hostsConfig hosts.HostsConfig, flake string, debug bool, doneCh chan struct{}) error {
	api := &API{
		ctx:         ctx,
		hostsConfig: hostsConfig,
		flake:       flake,
		debug:       debug,
		doneCh:      doneCh,
	}

	server := &http.Server{
		Addr:    ":5000",
		Handler: api.router(),
	}
	log.Info("starting API server", "address", server.Addr)

	go func() {
		<-ctx.Done()
		server.Shutdown(ctx)
	}()

	return server.ListenAndServe()
}

func extractClientIP(r *http.Request) string {
	ip, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}

	return ip
}
