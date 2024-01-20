# Traceroute (Golang)

**NOTE** This is an **experimental** fork that emits OpenTelemetry trace spans

## Fork

This was forked from https://github.com/mgranderath/traceroute and then updated.


This is an implementation of UDP (quic support) and TCP Traceroute in golang. 
It is specifically tailored to my use case for measurements and can be seen as an example for implementation.


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
  udp         UDP traceroute.
  tcp         TCP traceroute
  service     Run as a service
  generate    Generate a configuration file and print to stdout to run this as a service


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

### running as a service
```
$ traceroute service --help
Usage: traceroute service

Run as a service

Flags:
  -h, --help                  Show context-sensitive help.

      --config-file=STRING    Load a YAML configuration file ($TRACE_CFGFILE)
      --validate              Validate the configuration file format is correct and then exit
```

### generate empty configuration
```
$ traceroute generate --help
Usage: traceroute generate

Generate a configuration file and print to stdout to run this as a service

Flags:
  -h, --help    Show context-sensitive help.

```


## Docker

```
docker buildx build --attest type=sbom --platform linux/amd64 --tag jimmystewpot/traceroute:latest .
```

Running it as a service requires the configuration to be available via a local mount export.

```
docker run -v { path to configuration file}:/{ path to configuration inside container }/ \
      jimmystewpot/traceroute:latest service \
      --config-file=/{ path to configuration inside container }/config.yaml
```
