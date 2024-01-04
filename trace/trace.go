package trace

import (
	"context"
	"fmt"
	"os"
	"time"

	"github.com/alecthomas/kong"
	"github.com/jimmystewpot/traceroute/methods"
	"github.com/jimmystewpot/traceroute/methods/tcp"
	"github.com/jimmystewpot/traceroute/methods/udp"
	"github.com/rs/xid"
	"go.opentelemetry.io/otel"
)

type TraceConfig struct {
	MaxHops                  uint16        `help:"Set the maximum hops for the traceroute" short:"m" default:"30" env:"TRACE_MAXHOPS"`
	NQueries                 uint16        `help:"Set the number of probes per hop to send" short:"q" default:"3" env:"TRACE_NQUERIES"`
	ParallelRequests         uint16        `help:"Set maximum number of parallel requests in flight" short:"N" default:"16" env:"TRACE_PARALLEL"`
	Timeout                  time.Duration `help:"Set a timeout" short:"w" default:"2s" env:"TRACE_TIMEOUT"`
	TraceRoutePort           int           `help:"Set the port on which to traceroute" short:"p" default:"33434" env:"TRACE_SRC_PORT"`
	OpenTelemetryDestination string        `required:"" help:"OpenTelemetry destination to upload otel traces to" name:"otel-dest" default:"localhost" env:"TRACE_OTEL_DEST"`
	OpenTelemetryTLS         bool          `help:"OpenTelemetry destination requires TLS" name:"otel-tls" default:"false" env:"TRACE_OTEL_TLS"`
	OpenTelemetryGRPC        bool          `help:"OpenTelemetry uses GPRC protocol" name:"otel-grpc" default:"true" env:"TRACE_OTEL_GRPC"`
	OpenTelemetryPort        int           `help:"OpenTelemetry destination port to send traces to" name:"otel-port" default:"4317" env:"TRACE_OTEL_PORT"`
	Destination              string        `required:"" help:"IP or Hostname address to traceroute to" env:"TRACE_DESTINATION"`
	PrintResults             bool          `required:"" help:"Print the results to stdout, this is not recommended if running in docker" default:"false" env:"TRACE_STDOUT"`
	hostname                 string
}

func (tc *TraceConfig) UDP(destination string) {
		// exportTrace will export the spans when the tool quits.
		exportTrace, err := toc.initTraceProvider(toc.Timeout)
		if err != nil {
			return err
		}
		defer exportTrace()
	
}

func (tc *TraceConfig) TCP(destination string) {}


/*
// Run the OTEL traceroutes from the cmd line, i.e. not as a service.
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
	if toc.PrintResults {
		printResults(res)
	}
	return nil
}
*.
