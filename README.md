# Traceroute (Golang)

This is an implementation of UDP (quic support) and TCP Traceroute in golang. 
It is specifically tailored to my use case for measurements and can be seen as an example for implementation.

**NOTE** This is an experimental fork to have network distributed traceroutes emit otel traces.

## Building

```
go build -o traceroute main.go
```

## Using

```
$ traceroute --help
traceroute with otel distributed tracing

Flags:
  -h, --help    Show context-sensitive help.

Commands:
  udp    UDP traceroute.
  tcp    TCP traceroute

Run "traceroute --help" for more information on a command.

```
### udp traceroute
```
$ traceroute udp --help
Usage: traceroute udp --otel-dest="localhost" --destination=STRING

UDP traceroute.

Flags:
  -h, --help                      Show context-sensitive help.

  -m, --max-hops=30               Set the maximum hops for the traceroute ($TRACE_MAXHOPS)
  -q, --n-queries=3               Set the number of probes per hop to send ($TRACE_NQUERIES)
  -N, --parallel-requests=16      Set maximum number of parallel requests in flight ($TRACE_PARALLEL)
  -w, --timeout=2s                Set a timeout ($TRACE_TIMEOUT)
  -p, --trace-route-port=33434    Set the port on which to traceroute ($TRACE_SRC_PORT)
      --otel-dest="localhost"     OpenTelemetry destination to upload otel traces to ($TRACE_OTEL_DEST)
      --otel-tls                  OpenTelemetry destination requires TLS ($TRACE_OTEL_TLS)
      --otel-grpc                 OpenTelemetry uses GPRC protocol ($TRACE_OTEL_GRPC)
      --otel-port=4317            OpenTelemetry destination port to send traces to ($TRACE_OTEL_PORT)
      --destination=STRING        IP or Hostname address to traceroute to ($TRACE_DESTINATION)
      --print-results             Print the results to stdout, this is not recommended if running in docker ($TRACE_STDOUT)

```
### tcp traceroute
```
$ traceroute tcp --help
Usage: traceroute tcp --otel-dest="localhost" --destination=STRING

TCP traceroute

Flags:
  -h, --help                      Show context-sensitive help.

  -m, --max-hops=30               Set the maximum hops for the traceroute ($TRACE_MAXHOPS)
  -q, --n-queries=3               Set the number of probes per hop to send ($TRACE_NQUERIES)
  -N, --parallel-requests=16      Set maximum number of parallel requests in flight ($TRACE_PARALLEL)
  -w, --timeout=2s                Set a timeout ($TRACE_TIMEOUT)
  -p, --trace-route-port=33434    Set the port on which to traceroute ($TRACE_SRC_PORT)
      --otel-dest="localhost"     OpenTelemetry destination to upload otel traces to ($TRACE_OTEL_DEST)
      --otel-tls                  OpenTelemetry destination requires TLS ($TRACE_OTEL_TLS)
      --otel-grpc                 OpenTelemetry uses GPRC protocol ($TRACE_OTEL_GRPC)
      --otel-port=4317            OpenTelemetry destination port to send traces to ($TRACE_OTEL_PORT)
      --destination=STRING        IP or Hostname address to traceroute to ($TRACE_DESTINATION)
      --print-results             Print the results to stdout, this is not recommended if running in docker ($TRACE_STDOUT)

```

## Docker

```
docker buildx build --attest type=sbom --platform linux/amd64 --tag jimmystewpot/traceroute:latest .
```