// Package telemetry configures controller tracing.
package telemetry

import (
	"context"
	"os"
	"time"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/trace"
)

// Setup configures an OTLP exporter when an endpoint is configured and returns its shutdown function.
func Setup(ctx context.Context) (func(context.Context) error, error) {
	endpoint := os.Getenv("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT")
	if endpoint == "" {
		endpoint = os.Getenv("OTEL_EXPORTER_OTLP_ENDPOINT")
	}

	if endpoint == "" {
		return func(context.Context) error { return nil }, nil
	}

	service := os.Getenv("OTEL_SERVICE_NAME")
	if service == "" {
		service = "nixie"
	}

	exporter, err := otlptracegrpc.New(ctx)
	if err != nil {
		return nil, err
	}

	provider := sdktrace.NewTracerProvider(
		sdktrace.WithBatcher(exporter, sdktrace.WithBatchTimeout(time.Second)),
		sdktrace.WithResource(resource.NewWithAttributes("", attribute.String("service.name", service))),
	)
	otel.SetTracerProvider(provider)
	return provider.Shutdown, nil
}

// End records an operation error, if any, and ends its span.
func End(span trace.Span, err error) {
	if err != nil {
		span.RecordError(err)
		span.SetStatus(codes.Error, err.Error())
	}

	span.End()
}
