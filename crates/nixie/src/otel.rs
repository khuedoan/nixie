//! Telemetry setup.
//!
//! Mirrors the Go `setupTracing`: logs always go to stderr, and OpenTelemetry
//! trace export is enabled when an OTLP endpoint is configured. Spans are
//! batched and exported over gRPC, matching the Go `otlptracegrpc` exporter.

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::{BatchConfigBuilder, BatchSpanProcessor, SdkTracerProvider};
use opentelemetry_sdk::Resource;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::prelude::*;

/// How long the batch processor waits before flushing a partial batch, matching
/// the Go `sdktrace.WithBatchTimeout(time.Second)`.
const BATCH_TIMEOUT: Duration = Duration::from_secs(1);

static INITIALIZED: OnceLock<()> = OnceLock::new();
static TRACER_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();
/// The Tokio runtime that drives the gRPC exporter. Tonic's channel needs a
/// live reactor, so it must outlive the provider.
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Initialise logging (and, when configured, trace export). Safe to call more
/// than once; only the first call has an effect.
pub fn init(debug: bool) {
    INITIALIZED.get_or_init(|| {
        let default = if debug { "debug" } else { "info" };
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("nixie={default},nixie_agent={default}")));

        let subscriber = tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer());

        match otlp_endpoint() {
            Some(endpoint) => match build_provider(&endpoint) {
                Ok(provider) => {
                    let tracer = provider.tracer("nixie");
                    let _ = TRACER_PROVIDER.set(provider);
                    subscriber
                        .with(tracing_opentelemetry::layer().with_tracer(tracer))
                        .init();
                    tracing::debug!(%endpoint, "exporting traces over OTLP");
                }
                Err(error) => {
                    subscriber.init();
                    tracing::warn!(%error, "failed to configure OTLP exporter, traces disabled");
                }
            },
            None => subscriber.init(),
        }
    });
}

/// Flush any buffered spans and shut the exporter down. No-op when tracing is
/// disabled or already stopped.
pub fn shutdown() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        if let Err(error) = provider.shutdown() {
            tracing::warn!(%error, "failed to shut down telemetry");
        }
    }
}

fn build_provider(endpoint: &str) -> Result<SdkTracerProvider> {
    // Tonic's gRPC channel needs an active Tokio reactor, so build the exporter
    // inside a runtime and keep that runtime alive for the process lifetime.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .context("building Tokio runtime for the OTLP exporter")?;

    let provider = runtime.block_on(async {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .with_timeout(Duration::from_secs(5))
            .build()
            .context("building the OTLP span exporter")?;

        let processor = BatchSpanProcessor::builder(exporter)
            .with_batch_config(
                BatchConfigBuilder::default()
                    .with_scheduled_delay(BATCH_TIMEOUT)
                    .build(),
            )
            .build();

        let resource = Resource::builder()
            .with_attribute(KeyValue::new("service.name", service_name("nixie")))
            .build();

        Ok::<_, anyhow::Error>(
            SdkTracerProvider::builder()
                .with_span_processor(processor)
                .with_resource(resource)
                .build(),
        )
    })?;

    let _ = RUNTIME.set(runtime);
    Ok(provider)
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
