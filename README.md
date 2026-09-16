# Traceroute

A high-performance, asynchronous traceroute utility written in idiomatic Rust, featuring OpenTelemetry distributed tracing and continuous service monitoring capabilities.

This project is a fork of `jimmystewpot/traceroute`, modernised and adapted into a production-grade daemon.

---

## Overview

This utility provides network path discovery and latency analysis across IPv4 and IPv6 topologies utilising asynchronous UDP and TCP SYN probing. Engineered as a modern Rust successor to the original Go implementation, it delivers high throughput, deterministic resource utilisation, and robust distributed observability without runtime panics.

### Key Capabilities

- **Asynchronous Probing Engine:** Dispatches concurrent network probes powered by Tokio, strictly rate-limited through an asynchronous semaphore (`--parallel-requests`) to eliminate socket starvation and packet loss.
- **OpenTelemetry Observability:** Emits distributed trace spans for every hop traversal via the standard OpenTelemetry Protocol (OTLP) over gRPC or HTTP, complete with telemetry baggage (`device-name`, `destination-host`, `probe-protocol`) and optional linked spans correlating recurring runs.
- **Autonomous Service Daemon:** Runs continuously in the background, executing scheduled traces across configured destinations with customisable polling intervals and graceful signal-driven shutdown.
- **Embedded Health-Check Server:** Exposes an Axum HTTP endpoint (`/_healthcheck`) reporting real-time operational statistics and probe execution metrics formatted as JSON.
- **Deterministic Hop Reduction:** Accurately deduplicates and orders probe query responses, automatically terminating traversal upon reaching the final target destination.
- **Strict Configuration Validation:** Parses structured YAML configuration files against versioned schemas (`schema-version: 1.0.0`), failing fast on syntax discrepancies or unsupported specifications.

---

## Installation & Compilation

Ensure a current Rust toolchain (2021 edition, Rust 1.75 or newer) is installed on your operating system.

### Compiling the Binary

To compile an optimised release binary:

```bash
cargo build --release
```

The resulting executable is placed at `target/release/traceroute`.

### Running Verification Tests

To execute the unit and end-to-end integration test suite:

```bash
cargo test
```

---

## Command-Line Interface

The `traceroute` binary supports four primary subcommands:

```
Usage: traceroute <COMMAND>

Commands:
  udp       UDP traceroute mode
  tcp       TCP traceroute mode
  service   Run as a background service daemon
  generate  Generate a sample configuration file and output to stdout
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

### 1. UDP Traceroute (`traceroute udp`)

Dispatches UDP datagrams initialised with incremental Time-To-Live (TTL) values towards the target host.

```bash
traceroute udp --destination 1.1.1.1 [OPTIONS]
```

#### Supported Options & Environment Overrides

| Flag | Long Option | Environment Variable | Default | Description |
| :--- | :--- | :--- | :--- | :--- |
| `-m` | `--max-hops` | `TRACE_MAXHOPS` | `30` | Maximum network hops (TTL) to traverse |
| `-q` | `--n-queries` | `TRACE_NQUERIES` | `3` | Number of probe queries sent per hop |
| `-N` | `--parallel-requests` | `TRACE_PARALLEL` | `16` | Maximum concurrent probes in flight |
| `-w` | `--timeout` | `TRACE_TIMEOUT` | `2s` | Response timeout duration per probe |
| `-p` | `--trace-route-port` | `TRACE_SRC_PORT` | `33434` | Base destination UDP port |
| | `--destination` | `TRACE_DESTINATION` | *Required* | Hostname or IP address to trace |
| | `--print-results` | `TRACE_STDOUT` | `false` | Output hop traversal details to stdout |
| | `--otel-dest` | `TRACE_OTEL_DEST` | `localhost` | OpenTelemetry collector host |
| | `--otel-port` | `TRACE_OTEL_PORT` | `4317` | OpenTelemetry collector port |
| | `--otel-grpc` | `TRACE_OTEL_GRPC` | `true` | Export traces utilising gRPC (HTTP when false) |
| | `--otel-tls` | `TRACE_OTEL_TLS` | `false` | Enable TLS encryption for OTLP export |
| | `--otel-linked-spans` | `TRACEROUTE_OTEL_LINKED_SPANS` | `false` | Link consecutive trace iterations via OpenTelemetry span links |

#### Example

```bash
traceroute udp --destination example.com --max-hops 20 --n-queries 3 --print-results
```

---

### 2. TCP Traceroute (`traceroute tcp`)

Dispatches crafted TCP SYN packets with incremental TTL values, ideal for path analysis through firewalls that drop UDP traffic.

```bash
traceroute tcp --destination 1.1.1.1 [OPTIONS]
```

#### Supported Options & Environment Overrides

The TCP subcommand accepts the identical flags and environment variables as the UDP subcommand (including `--otel-linked-spans`). By convention, `--trace-route-port` defaults to standard service ports (e.g. port 80 or 443).

#### Example

```bash
traceroute tcp --destination example.com --trace-route-port 443 --print-results
```

---

### 3. Service Daemon (`traceroute service`)

Executes continuous traceroutes in daemon mode across multiple destinations defined within a YAML configuration file.

```bash
traceroute service --config-file <PATH> [OPTIONS]
```

#### Supported Options & Environment Overrides

| Flag | Long Option | Environment Variable | Default | Description |
| :--- | :--- | :--- | :--- | :--- |
| | `--config-file` | `TRACE_CFGFILE` | *Required* | Path to the YAML configuration file |
| | `--validate` | | `false` | Validate configuration syntax and exit |

#### Validating Configuration

To check whether a configuration file conforms to the required specification without initialising probes or starting the daemon:

```bash
traceroute service --config-file /etc/traceroute/config.yaml --validate
```

Upon successful validation, the command outputs `Configuration file validated successfully.` and exits with status 0.

#### Running the Daemon

```bash
traceroute service --config-file /etc/traceroute/config.yaml
```

#### Graceful Shutdown & Signal Handling

The service daemon captures POSIX termination signals (`SIGINT` / Ctrl+C and `SIGTERM`) to coordinate clean, non-disruptive process termination:
- **Active Probe Batch Draining:** When a termination signal is received, currently in-flight probe batches are permitted to drain completely before process exit, preventing half-open socket descriptors, orphaned network packets, or dropped results.
- **Health Server Connection Flushing:** The embedded Axum health server receives a broadcast cancellation signal, gracefully completing existing HTTP requests and flushing connection buffers before closing its TCP listener.
- **OpenTelemetry Buffer Flushing:** Distributed tracing providers explicitly flush all pending span buffers and cleanly shut down batch processors (`opentelemetry::global::shutdown_tracer_provider`), guaranteeing that all collected telemetry reaches remote OTLP collectors without data loss.

---

### 4. Configuration Template Generation (`traceroute generate`)

Generates a pre-populated, valid YAML configuration template adhering to `schema-version: 1.0.0` and prints it to standard output.

```bash
traceroute generate > config.yaml
```

---

## Configuration Specification

The configuration file is structured in YAML format according to version `1.0.0`.

### Complete Sample Configuration

```yaml
---
schema-version: 1.0.0
destinations:
  - first-test-domain.org
  - second-test-domain.org
globals:
  protocol: tcp
  max-hops: 60
  number-queries: 3
  parallel-requests: 16
  timeout: 5s
  source-port: 80
  interval: 1m
opentelemetry:
  destination: 127.0.0.1
  tls: false
  port: 4317
  grpc: true
  linked-spans: false
healthcheck:
  path: /_healthcheck
  enabled: true
  port: 8080
```

### Schema Field Reference

#### Root Fields

| Field | Type | Description |
| :--- | :--- | :--- |
| `schema-version` | String | Must be `"1.0.0"` to match supported schema version |
| `destinations` | Sequence of Strings | List of target hostnames or IP addresses to monitor |
| `globals` | Mapping | Global probing parameters and default options |
| `opentelemetry` | Mapping | OpenTelemetry collector connectivity parameters |
| `healthcheck` | Mapping | Axum health-check HTTP server configuration |

#### `globals` Mapping

| Field | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `protocol` | String | `"tcp"` | Probing protocol (`tcp` or `udp`) |
| `max-hops` | Integer | `60` | Maximum network hops before terminating probe traversal |
| `number-queries` | Integer | `3` | Number of probe attempts dispatched per hop |
| `parallel-requests` | Integer | `16` | Maximum concurrency limit for in-flight requests |
| `timeout` | Human duration | `5s` | Timeout duration before considering a probe dropped |
| `source-port` | Integer | `80` | Target or source port utilised for packet creation |
| `interval` | Human duration | `1m` | Rest period between consecutive monitoring cycles |

#### `opentelemetry` Mapping

| Field | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `destination` | String | `"127.0.0.1"` | Hostname or IP of the OpenTelemetry collector |
| `port` | Integer | `4317` | Port of the OpenTelemetry collector |
| `tls` | Boolean | `false` | Whether to establish TLS encryption for export |
| `grpc` | Boolean | `true` | When true uses gRPC transport; otherwise HTTP |
| `linked-spans` | Boolean | `false` | Link consecutive trace iterations to correlate recurring probes |

#### `healthcheck` Mapping

| Field | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `enabled` | Boolean | `true` | Whether to run the health-check HTTP server |
| `port` | Integer | `8080` | TCP port on which the HTTP server listens |
| `path` | String | `"/_healthcheck"` | URL route path exposing the JSON status payload |

---

## OpenTelemetry Distributed Tracing & Linked Spans

Every traceroute probe sequence exports structured OpenTelemetry spans via OTLP over gRPC or HTTP, integrating network path discovery and latency profiling into distributed tracing pipelines.

### Trace Span Hierarchy

- **Root Trace Span (`traceroute.trace`):** Spans the entire execution lifecycle across all hops towards a specific target. Key attributes include:
  - `destination`: Target IPv4 or IPv6 destination address.
  - `protocol`: Transport protocol utilised (`tcp` or `udp`).
  - `max_hops`: Maximum TTL threshold configured.
  - `queries_per_hop`: Number of probes dispatched per hop distance.
- **Hop Child Spans (`traceroute.hop`):** Each probe query records an individual child span parented to the root trace span. Recorded attributes include:
  - `hop.ttl`: Hop distance (Time-To-Live).
  - `query.index`: Zero-indexed probe attempt for the hop.
  - `hop.success`: Boolean indicating whether a valid response was received.
  - `router.ip`: IP address of the responding intermediate router or target host (if received).
  - `rtt_ms`: Observed round-trip latency in milliseconds.
- **Telemetry Baggage:** Contextual metadata items (`destination_hostname`, `source`, `max_hops`, `xid`) are propagated via OpenTelemetry baggage to facilitate end-to-end correlation across observability backends.

### Correlating Recurring Probes with Linked Spans

When operating as a background service daemon or running periodic health audits, repeated traceroutes target the same destination across successive polling intervals. Creating parent-child relationships across distinct scheduled cycles is an anti-pattern that bloats trace trees and distorts latency averages. Instead, OpenTelemetry **Span Links** provide a standardised causal link between independent trace cycles.

#### Enabling Linked Spans

- **CLI Flag:** Pass `--otel-linked-spans` (or set the `TRACEROUTE_OTEL_LINKED_SPANS=true` environment variable) when executing `traceroute udp` or `traceroute tcp`.
- **Daemon Configuration:** Set `linked-spans: true` under `opentelemetry:` in the YAML configuration file:

```yaml
opentelemetry:
  destination: 127.0.0.1
  port: 4317
  grpc: true
  tls: false
  linked-spans: true
```

#### APM & Observability Benefits

When `linked-spans` is enabled, the daemon maintains an in-memory cache of preceding span contexts keyed by target IP address. On each subsequent iteration targeting that destination, the new root span attaches an OpenTelemetry `Link` referencing the prior trace context:
- **Visualise Route Drift:** Modern APM platforms (such as Jaeger, Grafana Tempo, Honeycomb, and Datadog) render span links as clickable causal edges. Operators can traverse forward and backward across consecutive trace iterations to pinpoint the precise moment routing topology shifts or autonomous system (AS) path changes take place.
- **Analyse Latency Variations:** Causal links allow monitoring tools to correlate progressive latency degradation across intermediate hops over time without conflating separate polling cycles into single synthetic transactions.
- **Independent Span Lifecycles:** Each trace iteration maintains independent root start and end times, ensuring duration metrics, percentile latencies, and service level indicators (SLIs) remain accurate.

---

## Health-Check Monitoring Endpoint

When enabled in service mode, an Axum HTTP server initialises and listens on `0.0.0.0:<port>`.

### Request

```bash
curl -s http://localhost:8080/_healthcheck
```

### Response Payload

The endpoint responds with an `application/json` payload preserving compatibility with external monitoring systems:

```json
{
  "status": "ok",
  "service-name": "opentelemetry-traceroute",
  "hostname": "gateway-host-01",
  "current-time": "2026-09-15T18:30:00Z",
  "details": {
    "successful-traces": 128,
    "unsuccessful-traces": 2,
    "total-traces": 130,
    "dns-latency": [12, 14, 11]
  }
}
```

Unregistered paths fall back to an HTTP 400 Bad Request response with `invalid request\n`, replicating established behaviour.

---

## Containerisation & Docker Deployment

A lightweight container image can be built utilising multi-stage container builds.

### Building the Image

```bash
docker build -t jimmystewpot/traceroute:latest .
```

### Running the Container

Mount your local configuration file into the container:

```bash
docker run -d \
  --name traceroute-daemon \
  -v "$(pwd)/config.yaml:/etc/traceroute/config.yaml:ro" \
  -p 8080:8080 \
  jimmystewpot/traceroute:latest service \
  --config-file /etc/traceroute/config.yaml
```

---

## Engineering Standards

- **Memory Safety & Robustness:** Zero instances of `.unwrap()` or `.expect()` across all production library and binary entry paths. Errors are handled gracefully via `Result` types.
- **Deterministic Concurrency:** Concurrency is regulated through Tokio semaphores without unbounded thread creation or memory leaks.
- **Graceful Lifecycle Management:** Daemon mode coordinates non-disruptive termination across active trace batches, HTTP health-check connections, and OTLP exporter buffers upon `SIGINT` / `SIGTERM`.
- **OpenTelemetry Standards:** Compliant with OpenTelemetry semantic conventions, supporting distributed trace baggage and causal span linking across recurring iterations.

---

## Licence

Licensed under the Apache Licence, Version 2.0. See the `LICENSE` file for details.
