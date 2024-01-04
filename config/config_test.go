package config

import (
	"testing"
)

func TestTraceConfigCheckandSetValues(t *testing.T) {
	type fields struct {
		SchemaVersion           string
		TraceConfigDestinations []string
		TraceConfigGlobal       TraceConfigGlobal
		TraceConfigOtel         TraceConfigOtel
		TraceConfigHealthCheck  TraceConfigHealthCheck
	}
	tests := []struct {
		name    string
		fields  fields
		wantErr bool
	}{
		{
			name:    "empty configuration",
			fields:  fields{},
			wantErr: true,
		},
		{
			name: "missing timeout",
			fields: fields{
				SchemaVersion: schemaVersion,
				TraceConfigGlobal: TraceConfigGlobal{
					Timeout: 0,
				},
			},
			wantErr: false,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tc := &TraceConfig{
				SchemaVersion:           tt.fields.SchemaVersion,
				TraceConfigDestinations: tt.fields.TraceConfigDestinations,
				TraceConfigGlobal:       tt.fields.TraceConfigGlobal,
				TraceConfigOtel:         tt.fields.TraceConfigOtel,
				TraceConfigHealthCheck:  tt.fields.TraceConfigHealthCheck,
			}
			if err := tc.CheckandSetValues(); (err != nil) != tt.wantErr {
				t.Errorf("TraceConfig.CheckandSetValues() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}
