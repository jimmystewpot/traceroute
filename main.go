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
	Udp      trace.TraceCLI     `cmd:"" help:"UDP traceroute."`
	Tcp      trace.TraceCLI     `cmd:"" help:"TCP traceroute"`
	Service  service.ServiceCLI `cmd:"" help:"Run as a service"`
	Generate config.GenerateCLI `cmd:"" help:"Generate a configuration file and print to stdout to run this as a service"`
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
