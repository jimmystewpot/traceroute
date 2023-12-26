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
	"go.opentelemetry.io/otel/trace"
)

var cli struct {
	Udp TraceConfig `cmd:"" help:"UDP traceroute."`
	Tcp TraceConfig `cmd:"" help:"TCP traceroute"`
}

type TraceConfig struct {
	MaxHops                  uint16        `help:"Set the maximum hops for the traceroute" short:"m" default:"30"`
	NQueries                 uint16        `help:"Set the number of probes per hop to send" short:"q" default:"3"`
	ParallelRequests         uint16        `help:"Set maximum number of parallel requests in flight" short:"N" default:"16"`
	Timeout                  time.Duration `help:"Set a timeout" short:"w" default:"2s"`
	TraceRoutePort           int           `help:"Set the port on which to traceroute" short:"p" default:"33434"`
	OpenTelemetryDestination string        `required:"" help:"OpenTelemetry destination to upload otel traces to"`
	Destination              string        `required:"" help:"IP or Hostname address to traceroute to" default:"google.com"`
}

type TraceResults struct {
	Success bool
	Address string
	TTL     uint16
	RTT     *time.Duration
}

func (t *TraceConfig) Run(ctx *kong.Context) error {
	destinations, err := parseDestination(t.Destination)
	if err != nil {
		return err
	}
	cfg := methods.TracerouteConfig{
		MaxHops:          t.MaxHops,
		NumMeasurements:  3,
		ParallelRequests: t.ParallelRequests,
		Port:             t.TraceRoutePort,
		Timeout:          t.Timeout,
	}
	switch ctx.Command() {
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
		return fmt.Errorf("error command %s not understood", ctx.Command())
	}
	return nil
}

func (t *TraceConfig) SendTrace(res *map[uint16][]methods.TracerouteHop) error {
	hostname, err := os.Hostname()
	parentId := xid.New().String()
	if err != nil {
		return err
	}
	tracer := otel.Tracer(hostname)
	ctx := context.Background()
	ctx, parent := tracer.Start(
		ctx,
		parentId,
		trace.WithAttributes(
			attribute.String("hostname", hostname),
			attribute.String("destination", t.Destination),
		),
	)

	for i := uint16(0); i < uint16(len(*res)); i++ {
		if val, ok := (*res)[i]; ok {
			tr := checkNilResult((*res)[i])
			spanName := fmt.Sprintf("hop-%d", i)
			ctx, childSpan := tracer.Start(
				ctx,
				spanName,
				trace.WithAttributes(
					attribute.String("hostname", hostname),
					attribute.String("destination", t.Destination),
					attribute.Bool("success", tr[0].Success),
				),
			)
			for _, probes := range tr {
				childSpan.AddEvent(probes.Address)
			}

		}
	}
	return nil
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

func checkNilResult(results []methods.TracerouteHop) []TraceResults {
	tr := make([]TraceResults, 0)
	for _, hop := range results {
		t := TraceResults{
			RTT:     hop.RTT,
			Success: hop.Success,
			TTL:     hop.TTL,
		}
		if hop.Address == nil {
			t.Address = "Null"
		} else {
			t.Address = hop.Address.String()
		}
		tr = append(tr, t)
	}
	return tr
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
