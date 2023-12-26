package main

import (
	"context"
	"fmt"
	"net"
	"net/netip"
	"os"
	"time"

	"github.com/alecthomas/kong"
	"github.com/mgranderath/traceroute/methods"
	"github.com/mgranderath/traceroute/methods/tcp"
	"github.com/mgranderath/traceroute/methods/udp"
	"github.com/rs/xid"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/baggage"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	semconv "go.opentelemetry.io/otel/semconv/v1.4.0"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

var cli struct {
	Udp TraceOtelConfig `cmd:"" help:"UDP traceroute."`
	Tcp TraceOtelConfig `cmd:"" help:"TCP traceroute"`
}

type TraceOtelConfig struct {
	MaxHops                  uint16        `help:"Set the maximum hops for the traceroute" short:"m" default:"30"`
	NQueries                 uint16        `help:"Set the number of probes per hop to send" short:"q" default:"3"`
	ParallelRequests         uint16        `help:"Set maximum number of parallel requests in flight" short:"N" default:"16"`
	Timeout                  time.Duration `help:"Set a timeout" short:"w" default:"2s"`
	TraceRoutePort           int           `help:"Set the port on which to traceroute" short:"p" default:"33434"`
	OpenTelemetryDestination string        `required:"" help:"OpenTelemetry destination to upload otel traces to" name:"otel-dest" default:"localhost"`
	OpenTelemetryTLS         bool          `help:"OpenTelemetry destination requires TLS" name:"otel-tls" default:"false"`
	OpenTelemetryGRPC        bool          `help:"OpenTelemetry uses GPRC protocol" name:"otel-grpc" default:"true"`
	OpenTelemetryPort        int           `help:"OpenTelemetry destination port to send traces to" name:"otel-port" default:"4317"`
	Destination              string        `required:"" help:"IP or Hostname address to traceroute to" default:"google.com"`
	hostname                 string
}

func (toc *TraceOtelConfig) Run(kongctx *kong.Context) error {
	var err error
	toc.hostname, err = os.Hostname()
	if err != nil {
		return err
	}

	destinations, err := parseDestination(toc.Destination)
	if err != nil {
		return err
	}

	// exportTrace will export the spans when the tool quits.
	exportTrace, err := toc.initTraceProvider(toc.Timeout)
	if err != nil {
		return err
	}
	defer exportTrace()
	ctx := context.Background()
	// ctx is reset with the baggage added.
	ctx, err = toc.initBaggage(ctx)
	if err != nil {
		return err
	}

	cfg := methods.TracerouteConfig{
		DestinationHostname: toc.Destination,
		LocalHostname:       toc.hostname,
		MaxHops:             toc.MaxHops,
		NumMeasurements:     3,
		ParallelRequests:    toc.ParallelRequests,
		Port:                toc.TraceRoutePort,
		Timeout:             toc.Timeout,
		Tracer:              otel.Tracer(fmt.Sprintf("%s/traceroute", toc.hostname)),
		Xid:                 xid.New(),
		TraceCtx:            ctx,
	}

	switch kongctx.Command() {
	case "tcp":
		for i := 0; i < len(destinations); i++ {
			tcpTraceroute := tcp.New(destinations[i], cfg)
			res, err := tcpTraceroute.Start()
			if err != nil {
				return err
			}
			printResults(res)
		}

	case "udp":
		for i := 0; i < len(destinations); i++ {
			tcpTraceroute := udp.New(destinations[i], true, cfg)
			res, err := tcpTraceroute.Start()
			if err != nil {
				return err
			}
			printResults(res)
		}
	default:
		return fmt.Errorf("error command %s not understood", kongctx.Command())
	}
	return nil
}

// initBaggage will include the attributes globally for all spans. This works on some
// otel recivers but not all.
func (toc *TraceOtelConfig) initBaggage(ctx context.Context) (context.Context, error) {
	bag := baggage.FromContext(ctx)
	// set the global destination hostname for the traceroutes to be included in all spans.
	dest, err := baggage.NewMember("destination_hostnane", toc.Destination)
	if err != nil {
		return ctx, err
	}
	bag, _ = bag.SetMember(dest)

	// set the source hostname for all traceroutes to be included in all spans.
	src, err := baggage.NewMember("source", toc.hostname)
	if err != nil {
		return ctx, err
	}
	bag, _ = bag.SetMember(src)

	// set the MAX TTL for all traceroutes to be included in all spans
	maxTTL, err := baggage.NewMember("max_hops", fmt.Sprintf("%d", toc.MaxHops))
	if err != nil {
		return ctx, err
	}
	bag, _ = bag.SetMember(maxTTL)

	// set additional entropy using xid for rebuilding spans if required
	id := xid.New()
	xid, err := baggage.NewMember("xid", id.String())
	if err != nil {
		return ctx, err
	}
	bag, _ = bag.SetMember(xid)

	return baggage.ContextWithBaggage(ctx, bag), nil
}

// initTraceProvider is instantiated early and then run as the final function to export the trace.
func (toc *TraceOtelConfig) initTraceProvider(timeout time.Duration) (func(), error) {
	ctx, cancel := context.WithTimeout(context.Background(), timeout*time.Second)

	res, err := resource.New(ctx,
		resource.WithAttributes(
			semconv.ServiceNameKey.String(fmt.Sprintf("%s/traceroute", toc.hostname)),
			attribute.String("application", "otel-distributed-network-traceroute"),
		),
	)
	if err != nil {
		cancel()
		return func() {}, err
	}
	var exporter *otlptrace.Exporter
	dst := net.JoinHostPort(toc.OpenTelemetryDestination, fmt.Sprintf("%d", toc.OpenTelemetryPort))

	switch toc.OpenTelemetryGRPC {
	case true:
		// GRPC Destination Configuration for the exporter
		conn, err := grpc.DialContext(ctx, dst, grpc.WithTransportCredentials(insecure.NewCredentials()), grpc.WithBlock())
		if err != nil {
			cancel()
			return func() {}, err
		}

		exporter, err = otlptracegrpc.New(ctx, otlptracegrpc.WithGRPCConn(conn))
		if err != nil {
			cancel()
			return func() {}, err
		}
	case false:
		// HTTP Destination configuration for the exporter
		exporter, err = otlptracehttp.New(ctx)
		if err != nil {
			cancel()
			return func() {}, err
		}
	}

	batchSpanProcessor := sdktrace.NewBatchSpanProcessor(exporter)
	tracerProvider := sdktrace.NewTracerProvider(
		sdktrace.WithSampler(sdktrace.AlwaysSample()),
		sdktrace.WithResource(res),
		sdktrace.WithSpanProcessor(batchSpanProcessor),
	)
	otel.SetTracerProvider(tracerProvider)

	return func() {
		// Shutdown will flush any remaining spans and shut down the exporter.
		fmt.Printf("flushing TracerProvider")
		err := tracerProvider.Shutdown(ctx)
		if err != nil {
			fmt.Printf("error flushing TracerProvider: %s", err)
		}
		cancel()
	}, nil
}

// printResults will print out the results line by line for easy reading.
func printResults(res *map[uint16][]methods.TracerouteHop) error {
	for i := uint16(0); i < uint16(len(*res)); i++ {
		if val, ok := (*res)[i]; ok {
			fmt.Println(i, val)
		}
	}
	return nil
}

// parseDestination takes a string hostname and returns the IP addresses or handles an error.
func parseDestination(destination string) ([]net.IP, error) {
	res, err := net.LookupIP(destination)
	if err != nil {
		return nil, err
	}
	if len(res) == 0 {
		return nil, fmt.Errorf("destination %s failed to resolve", destination)
	}
	results := make([]net.IP, 0)
	for i := 0; i < len(res); i++ {
		ip, err := netip.ParseAddr(res[i].String())
		if err != nil {
			return nil, err
		}
		if ip.Is4() {
			results = append(results, res[i])
		}
	}
	return results, nil
}

func main() {
	ctx := kong.Parse(&cli,
		kong.Name(os.Args[0]),
		kong.Description("traceroute with otel distributed tracing"),
		kong.UsageOnError(),
		kong.ConfigureHelp(kong.HelpOptions{
			Compact: true,
		}),
	)
	err := ctx.Run(ctx)
	if err != nil {
		fmt.Printf("error: %s\n", err)
	}
}
