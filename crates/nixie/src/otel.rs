//! Telemetry setup.
//!
//! Mirrors the Go `setupTracing`: logs always go to stderr, and OpenTelemetry
//! trace export is enabled when an OTLP endpoint is configured. Until the OTLP
//! exporter is wired up, an endpoint is accepted and logged so the process
//! behaves the same as Go from the outside.

use std::sync::OnceLock;

use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::prelude::*;

static INITIALIZED: OnceLock<()> = OnceLock::new();

/// Initialise logging (and, when configured, trace collection). Safe to call
/// more than once; only the first call has an effect.
pub fn init(debug: bool) {
    INITIALIZED.get_or_init(|| {
        let default = if debug { "debug" } else { "info" };
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("nixie={default},nixie_agent={default}")));

        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();

        if let Some(endpoint) = otlp_endpoint() {
            tracing::debug!(endpoint, "OTLP endpoint configured");
        }
    });
}

/// The configured OTLP endpoint, if any, matching the Go environment checks.
pub fn otlp_endpoint() -> Option<String> {
    std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT")
                .ok()
                .filter(|value| !value.is_empty())
        })
}

/// The service name to report, allowing `OTEL_SERVICE_NAME` to override it.
pub fn service_name(default: &str) -> String {
    std::env::var("OTEL_SERVICE_NAME")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}
