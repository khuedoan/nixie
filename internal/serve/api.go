package serve

import (
	"context"
	"io"
	"net"
	"net/http"

	"github.com/charmbracelet/log"
)

func extractClientIP(r *http.Request) string {
	ip, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}

	return ip
}

func handlePing(w http.ResponseWriter, r *http.Request) {
	log.Info("received ping from agent", "ip", extractClientIP(r))
	io.WriteString(w, "pong")
}

func StartAPIServer(ctx context.Context, debug bool) error {
	http.HandleFunc("GET /ping", handlePing)

	return http.ListenAndServe(":5000", nil)
}
