package main

import (
	"fmt"
	"os"

	"github.com/alecthomas/kong"
	"github.com/jimmystewpot/traceroute/config"
	"github.com/jimmystewpot/traceroute/service"
	"github.com/jimmystewpot/traceroute/trace"
)

var cli struct {
	UDP      trace.CLI   `cmd:"" help:"UDP traceroute."`
	TCP      trace.CLI   `cmd:"" help:"TCP traceroute"`
	Service  service.CLI `cmd:"" help:"Run as a service"`
	Generate config.CLI  `cmd:"" help:"Generate a configuration file and print to stdout to run this as a service"`
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
