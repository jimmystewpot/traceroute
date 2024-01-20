package trace

import (
	"net"
	"testing"
	"time"
)

func TestParseDestination(t *testing.T) {
	type args struct {
		destination string
	}
	tests := []struct {
		name    string
		args    args
		want    []net.IP
		wantErr bool
	}{
		{
			name: "validate destination",
			args: args{
				destination: "google.com",
			},
			wantErr: false,
		},
		{
			name: "invalidate destination",
			args: args{
				destination: "----.com",
			},
			wantErr: true,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := parseDestination(tt.args.destination)
			if (err != nil) != tt.wantErr {
				t.Errorf("parseDestination() error = %v, wantErr %v", err, tt.wantErr)
				return
			}
		})
	}
}

func TestCLITCP(t *testing.T) {
	type fields struct {
		MaxHops                  uint16
		NQueries                 uint16
		ParallelRequests         uint16
		Timeout                  time.Duration
		TraceRoutePort           int
		OpenTelemetryDestination string
		OpenTelemetryTLS         bool
		OpenTelemetryGRPC        bool
		OpenTelemetryPort        int
		Destination              string
		PrintResults             bool
		Hostname                 string
	}
	tests := []struct {
		name    string
		fields  fields
		wantErr bool
	}{
		{
			name: "parse data until root socket error http",
			fields: fields{
				Destination:    "localhost",
				Hostname:       "localhost",
				Timeout:        1 * time.Second,
				TraceRoutePort: 8000,
			},
			wantErr: true,
		},
		{
			name: "parse data until root socket error grpc",
			fields: fields{
				Destination:       "localhost",
				Hostname:          "localhost",
				Timeout:           1 * time.Second,
				TraceRoutePort:    8000,
				OpenTelemetryGRPC: true,
			},
			wantErr: true,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cli := &CLI{
				MaxHops:                  tt.fields.MaxHops,
				NQueries:                 tt.fields.NQueries,
				ParallelRequests:         tt.fields.ParallelRequests,
				Timeout:                  tt.fields.Timeout,
				TraceRoutePort:           tt.fields.TraceRoutePort,
				OpenTelemetryDestination: tt.fields.OpenTelemetryDestination,
				OpenTelemetryTLS:         tt.fields.OpenTelemetryTLS,
				OpenTelemetryGRPC:        tt.fields.OpenTelemetryGRPC,
				OpenTelemetryPort:        tt.fields.OpenTelemetryPort,
				Destination:              tt.fields.Destination,
				PrintResults:             tt.fields.PrintResults,
				Hostname:                 tt.fields.Hostname,
			}
			if err := cli.TCP(); (err != nil) != tt.wantErr {
				t.Errorf("CLI.TCP() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestCLIUDP(t *testing.T) {
	type fields struct {
		MaxHops                  uint16
		NQueries                 uint16
		ParallelRequests         uint16
		Timeout                  time.Duration
		TraceRoutePort           int
		OpenTelemetryDestination string
		OpenTelemetryTLS         bool
		OpenTelemetryGRPC        bool
		OpenTelemetryPort        int
		Destination              string
		PrintResults             bool
		Hostname                 string
	}
	tests := []struct {
		name    string
		fields  fields
		wantErr bool
	}{
		{
			name: "parse data until root socket error http",
			fields: fields{
				Destination:    "localhost",
				Hostname:       "localhost",
				Timeout:        1 * time.Second,
				TraceRoutePort: 8000,
			},
			wantErr: true,
		},
		{
			name: "parse data until root socket error grpc",
			fields: fields{
				Destination:       "localhost",
				Hostname:          "localhost",
				Timeout:           1 * time.Second,
				TraceRoutePort:    8000,
				OpenTelemetryGRPC: true,
			},
			wantErr: true,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cli := &CLI{
				MaxHops:                  tt.fields.MaxHops,
				NQueries:                 tt.fields.NQueries,
				ParallelRequests:         tt.fields.ParallelRequests,
				Timeout:                  tt.fields.Timeout,
				TraceRoutePort:           tt.fields.TraceRoutePort,
				OpenTelemetryDestination: tt.fields.OpenTelemetryDestination,
				OpenTelemetryTLS:         tt.fields.OpenTelemetryTLS,
				OpenTelemetryGRPC:        tt.fields.OpenTelemetryGRPC,
				OpenTelemetryPort:        tt.fields.OpenTelemetryPort,
				Destination:              tt.fields.Destination,
				PrintResults:             tt.fields.PrintResults,
				Hostname:                 tt.fields.Hostname,
			}
			if err := cli.UDP(); (err != nil) != tt.wantErr {
				t.Errorf("CLI.TCP() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}
