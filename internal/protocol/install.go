// Package protocol defines the installer agent's status messages.
package protocol

type InstallStatus string

const (
	StatusStarted   InstallStatus = "started"
	StatusCompleted InstallStatus = "completed"
	StatusFailed    InstallStatus = "failed"
)

// InstallResult reports startup or the terminal installation result.
type InstallResult struct {
	MACAddress string        `json:"mac_address"`
	Status     InstallStatus `json:"status"`
	Error      string        `json:"error,omitempty"`
}
