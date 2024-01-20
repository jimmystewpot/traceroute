package trace

import (
	"context"
	"fmt"
	"net"
	"net/netip"
	"os"
	"time"

	"github.com/alecthomas/kong"
	"github.com/jimmystewpot/traceroute/methods"
	"github.com/jimmystewpot/traceroute/methods/tcp"
	"github.com/jimmystewpot/traceroute/methods/udp"
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

const (
	tracerName      string = "%s/traceroute"
	applicationName string = "github.com/jimmystewpot/traceroute"
)

type CLI struct {
	MaxHops                  uint16        `help:"Set the maximum hops for the traceroute" short:"m" default:"30" env:"TRACE_MAXHOPS"`
	NQueries                 uint16        `help:"Set the number of probes per hop to send" short:"q" default:"3" env:"TRACE_NQUERIES"`
	ParallelRequests         uint16        `help:"Set maximum number of parallel requests in flight" short:"N" default:"16" env:"TRACE_PARALLEL"`
	Timeout                  time.Duration `help:"Set a timeout" short:"w" default:"2s" env:"TRACE_TIMEOUT"`
	TraceRoutePort           int           `help:"Set the port on which to traceroute" short:"p" default:"33434" env:"TRACE_SRC_PORT"`
	OpenTelemetryDestination string        `required:"" help:"OpenTelemetry destination for traces" name:"otel-dest" default:"localhost" env:"TRACE_OTEL_DEST"`
	OpenTelemetryTLS         bool          `help:"OpenTelemetry destination requires TLS" name:"otel-tls" default:"false" env:"TRACE_OTEL_TLS"`
	OpenTelemetryGRPC        bool          `help:"OpenTelemetry uses GPRC protocol" name:"otel-grpc" default:"true" env:"TRACE_OTEL_GRPC"`
	OpenTelemetryPort        int           `help:"OpenTelemetry destination port to send traces to" name:"otel-port" default:"4317" env:"TRACE_OTEL_PORT"`
	Destination              string        `required:"" help:"IP or Hostname address to traceroute to" env:"TRACE_DESTINATION"`
	PrintResults             bool          `required:"" help:"Print trace to stdout, NOT recommended if running in docker" default:"false" env:"TRACE_STDOUT"`
	Hostname                 string        `hidden:""`
}

func (cli *CLI) Run(kongctx *kong.Context) error {
	var err error
	cli.Hostname, err = os.Hostname()
	if err != nil {
		return err
	}

	destinations, err := parseDestination(cli.Destination)
	if err != nil {
		return err
	}

	// exportTrace will export the spans when the tool quits.
	exportTrace, err := cli.initTraceProvider(cli.Timeout)
	if err != nil {
		return err
	}
	defer exportTrace()

	ctx := context.Background()
	// ctx is reset with the baggage added.
	ctx, err = cli.initBaggage(ctx)
	if err != nil {
		return err
	}

	cfg := cli.translateConfig(ctx)

	var res *map[uint16][]methods.TracerouteHop
	switch kongctx.Command() {
	case "tcp":
		for i := 0; i < len(destinations); i++ {
			tcpTraceroute := tcp.New(destinations[i], cfg)
			res, err = tcpTraceroute.Start()
		}

	case "udp":
		for i := 0; i < len(destinations); i++ {
			tcpTraceroute := udp.New(destinations[i], true, cfg)
			res, err = tcpTraceroute.Start()
		}
	default:
		return fmt.Errorf("error command %s not understood", kongctx.Command())
	}

	// checks error from within switch statement
	if err != nil {
		return err
	}
	if cli.PrintResults {
		printResults(res)
	}
	return nil
}

// UDP is used by the Service UDP traceroute system, it will generate a trace per destination.
func (cli *CLI) UDP() error {
	destinations, err := parseDestination(cli.Destination)
	if err != nil {
		return err
	}
	// exportTrace will export the spans when the tool quits.
	exportTrace, err := cli.initTraceProvider(cli.Timeout)
	if err != nil {
		return err
	}
	defer exportTrace()

	ctx := context.Background()
	// ctx is reset with the baggage added.
	ctx, err = cli.initBaggage(ctx)
	if err != nil {
		return err
	}

	cfg := cli.translateConfig(ctx)

	var res *map[uint16][]methods.TracerouteHop
	for i := 0; i < len(destinations); i++ {
		udpTraceroute := udp.New(destinations[i], true, cfg)
		res, err = udpTraceroute.Start()
		if cli.PrintResults {
			printResults(res)
		}
	}
	if err != nil {
		return err
	}

	return nil
}

// TCP is used by the Service TCP traceroute system, it will generate a trace per destination.
func (cli *CLI) TCP() error {
	destinations, err := parseDestination(cli.Destination)
	if err != nil {
		return err
	}
	// exportTrace will export the spans when the tool quits.
	exportTrace, err := cli.initTraceProvider(cli.Timeout)
	if err != nil {
		return err
	}
	defer exportTrace()

	ctx := context.Background()
	// ctx is reset with the baggage added.
	ctx, err = cli.initBaggage(ctx)
	if err != nil {
		return err
	}

	cfg := cli.translateConfig(ctx)

	var res *map[uint16][]methods.TracerouteHop
	for i := 0; i < len(destinations); i++ {
		for i := 0; i < len(destinations); i++ {
			tcpTraceroute := tcp.New(destinations[i], cfg)
			res, err = tcpTraceroute.Start()
		}
		if cli.PrintResults {
			printResults(res)
		}
	}
	if err != nil {
		return err
	}

	return nil
}

// translateConfig makes the configuration compatible with the root traceroute fork
func (cli *CLI) translateConfig(ctx context.Context) methods.TracerouteConfig {
	return methods.TracerouteConfig{
		DestinationHostname: cli.Destination,
		LocalHostname:       cli.Hostname,
		MaxHops:             cli.MaxHops,
		NumMeasurements:     3,
		ParallelRequests:    cli.ParallelRequests,
		Port:                cli.TraceRoutePort,
		Timeout:             cli.Timeout,
		Tracer:              otel.Tracer(fmt.Sprintf(tracerName, cli.Hostname)),
		Xid:                 xid.New(),
		TraceCtx:            ctx,
	}
}

// initBaggage will include the attributes globally for all spans. This works on some
// otel recivers but not all.
func (cli *CLI) initBaggage(ctx context.Context) (context.Context, error) {
	bag := baggage.FromContext(ctx)
	// set the global destination hostname for the traceroutes to be included in all spans.
	dest, err := baggage.NewMember("destination_hostnane", cli.Destination)
	if err != nil {
		return ctx, err
	}
	bag, _ = bag.SetMember(dest)

	// set the source hostname for all traceroutes to be included in all spans.
	src, err := baggage.NewMember("source", cli.Hostname)
	if err != nil {
		return ctx, err
	}
	bag, _ = bag.SetMember(src)

	// set the MAX TTL for all traceroutes to be included in all spans
	maxTTL, err := baggage.NewMember("max_hops", fmt.Sprintf("%d", cli.MaxHops))
	if err != nil {
		return ctx, err
	}
	bag, _ = bag.SetMember(maxTTL)

	// set additional entropy using xid for rebuilding spans if required
	id, err := baggage.NewMember("xid", xid.New().String())
	if err != nil {
		return ctx, err
	}
	bag, _ = bag.SetMember(id)

	return baggage.ContextWithBaggage(ctx, bag), nil
}

// initTraceProvider is instantiated early and then run as the final function to export the trace.
func (cli *CLI) initTraceProvider(timeout time.Duration) (func(), error) {
	ctx, cancel := context.WithTimeout(context.TODO(), timeout*time.Second)

	res, err := resource.New(ctx,
		resource.WithAttributes(
			semconv.ServiceNameKey.String(fmt.Sprintf("%s/traceroute", cli.Hostname)),
			attribute.String("application", applicationName),
		),
	)
	if err != nil {
		cancel()
		return func() {}, err
	}
	var exporter *otlptrace.Exporter
	dst := net.JoinHostPort(cli.OpenTelemetryDestination, fmt.Sprintf("%d", cli.OpenTelemetryPort))

	switch cli.OpenTelemetryGRPC {
	case true:
		// GRPC Destination Configuration for the exporter
		conn, gerr := grpc.DialContext(ctx, dst, grpc.WithTransportCredentials(insecure.NewCredentials()), grpc.WithUserAgent(applicationName))
		if gerr != nil {
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
		fmt.Printf("flushing TracerProvider to otel server %s", dst)
		err := tracerProvider.Shutdown(ctx)
		if err != nil {
			fmt.Printf("error flushing TracerProvider: %s", err)
		}
		cancel()
	}, nil
}

// printResults will print out the results line by line for easy reading.
func printResults(res *map[uint16][]methods.TracerouteHop) {
	for i := uint16(0); i < uint16(len(*res)); i++ {
		if val, ok := (*res)[i]; ok {
			fmt.Println(i, val)
		}
	}
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
