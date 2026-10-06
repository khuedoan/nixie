//! End-to-end check that spans produced through [`nixie::otel`] reach an OTLP
//! collector.
//!
//! Ignored by default because it needs a running collector. Run it with:
//!
//! ```sh
//! nix shell nixpkgs#opentelemetry-collector -c otelcol --config tests/otelcol.yaml &
//! OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:14317 \
//! OTEL_TRACE_FILE=trace.json \
//!   cargo test -p nixie --test otel_export -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires a running OTLP collector (see tests/otelcol.yaml)"]
fn spans_are_exported_to_the_collector() {
    let endpoint = match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(value) if !value.is_empty() => value,
        _ => {
            eprintln!("OTEL_EXPORTER_OTLP_ENDPOINT is not set, skipping");
            return;
        }
    };
    let trace_file = match std::env::var("OTEL_TRACE_FILE") {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => {
            eprintln!("OTEL_TRACE_FILE is not set, skipping");
            return;
        }
    };

    nixie::otel::init(true);

    {
        let span = tracing::info_span!(
            target: "nixie",
            "otel_export_test",
            "nixie.installer" = %endpoint,
            "test.attr" = "hello",
        );
        span.in_scope(|| tracing::info!(target: "nixie", "emitting a test span"));
    }

    nixie::otel::shutdown();

    // The collector's file exporter flushes asynchronously; give it a moment.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let contents = std::fs::read_to_string(&trace_file).unwrap_or_default();
        if contents.contains("otel_export_test") {
            assert!(
                contents.contains("nixie"),
                "exported span is missing the service name"
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "no spans were written to {}",
            trace_file.display()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}
