## OpenTelemetry exporting (since v20.2.0)

(enterprise-only)

### Configuring OpenTelemetry exporter

If OTEL_EXPORTER_OTLP_ENDPOINT is not set, Unleash Edge will log to stdout, obeying RUST_LOG settings as well as the
LOG_FORMAT (json / pretty / plain) setting

#### GRPC

To have Unleash Edge export tracing and logs to an OTLP/GRPC compatible collector, set the CLI Arg
`--otel-exporter-otlp-endpoint` or env var `OTEL_EXPORTER_OTLP_ENDPOINT` to the URL of your collector.
With a default collector listening for GRPC on port 4317, set this to `http://localhost:4317`

#### HTTP (Json)

At this time Edge only supports HTTP/JSON as an HTTP exporter. We do not support HTTP/Protobuf.

To have Unleash Edge export tracing and logs to an HTTP/JSON compatible collector, you'll need to set two variables:
`--otel-exporter-otlp-endpoint / OTEL_EXPORTER_OTLP_ENDPOINT` (default collector listens at http://localhost:4318)
`--otel-exporter-otlp-protocol / OTEL_EXPORTER_OTLP_PROTOCOL`. (set this to `http`)

### Further customizations

#### Different endpoints for tracing and logs

Not currently supported.

#### Custom headers

See
the [OTLP docs](https://opentelemetry.io/docs/specs/otel/protocol/exporter/#specifying-headers-via-environment-variables)

For the current implementation:

* OTEL_EXPORTER_OTLP_HEADERS will add headers for tracing and logs in W3C Baggage format
* OTEL_EXPORTER_OTLP_TRACES_HEADERS will add headers for tracing only
* OTEL_EXPORTER_OTLP_LOGS_HEADERS will add headers for logs only
