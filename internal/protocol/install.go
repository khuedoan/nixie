// Package protocol defines the installer agent's status messages.
package protocol

import "time"

type InstallStatus string

const (
	StatusStarted   InstallStatus = "started"
	StatusProgress  InstallStatus = "progress"
	StatusCompleted InstallStatus = "completed"
	StatusFailed    InstallStatus = "failed"
)

// InstallPhase identifies a completed operation measured by the agent.
type InstallPhase string

const (
	PhaseBoot          InstallPhase = "boot"
	PhaseBuildDisko    InstallPhase = "build_disko"
	PhasePartition     InstallPhase = "partition"
	PhaseInstallSystem InstallPhase = "install_system"
	PhaseReboot        InstallPhase = "reboot"
)

// InstallResult reports installation status and optional phase timing.
type InstallResult struct {
	MACAddress string        `json:"mac_address"`
	Status     InstallStatus `json:"status"`
	Phase      InstallPhase  `json:"phase,omitempty"`
	Duration   time.Duration `json:"duration_ns,omitempty"`
	Error      string        `json:"error,omitempty"`
}
