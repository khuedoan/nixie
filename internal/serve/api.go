package serve

import (
	"context"
	"io"
	"net"
	"net/http"

	"code.khuedoan.com/nixie/internal/hosts"
	"github.com/charmbracelet/log"
)

type API struct {
	hostsConfig hosts.HostsConfig
}

func (api *API) ping(w http.ResponseWriter, r *http.Request) {
	log.Info("received ping from agent", "ip", extractClientIP(r))
	io.WriteString(w, "pong")
}

func (api *API) router() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /ping", api.ping)

	return mux
}

func StartAPIServer(ctx context.Context, hostsConfig hosts.HostsConfig, debug bool) error {
	api := &API{
		hostsConfig: hostsConfig,
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
