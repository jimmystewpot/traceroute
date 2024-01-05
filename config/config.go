package config

import (
	"fmt"
	"io"
	"os"
	"time"

	"github.com/go-playground/validator/v10"
	"gopkg.in/yaml.v3"
)

const (
	schemaVersion string = "1.0.0"
	// if values are not defined in the configuration file, these are the defaults.
	defaultParallelRequests uint16        = 16
	defaultProtocol         string        = "tcp"
	defaultMaxHops          uint16        = 60
	defaultNumberQueries    uint16        = 3
	defaultTracePort        int           = 80
	defaultInterval         time.Duration = 60 * time.Second
	defaultTimeout          time.Duration = 5 * time.Second
)

var (
	validate *validator.Validate
)

type TraceConfig struct {
	SchemaVersion           string                 `yaml:"schema-version" validate:"semver,required"`
	TraceConfigDestinations []string               `yaml:"destinations" validate:"dive,fqdn"`
	TraceConfigGlobal       TraceConfigGlobal      `yaml:"globals"`
	TraceConfigOtel         TraceConfigOtel        `yaml:"opentelemetry"`
	TraceConfigHealthCheck  TraceConfigHealthCheck `yaml:"healthcheck"`
}

type TraceConfigGlobal struct {
	Protocol         string        `yaml:"protocol" validate:"oneof=udp tcp"`
	MaxHops          uint16        `yaml:"max-hops"`
	NQueries         uint16        `yaml:"number-queries"`
	ParallelRequests uint16        `yaml:"parallel-requests"`
	Timeout          time.Duration `yaml:"timeout"`
	TraceRoutePort   int           `yaml:"source-port"`
	Interval         time.Duration `yaml:"interval"`
}

type TraceConfigOtel struct {
	Destination string `yaml:"destination" validate:"required"`
	TLS         bool   `yaml:"tls"`
	Port        int    `yaml:"port"`
	GRPC        bool   `yaml:"grpc"`
}

type TraceConfigHealthCheck struct {
	Path    string `yaml:"path"`
	Enabled bool   `yaml:"enabled"`
	Port    int    `yaml:"port"`
}

type CLI struct{}

func (cli *CLI) Run() error {
	err := PrintEmptyConfiguration()
	if err != nil {
		return err
	}
	return nil
}

// ReadConfig wil read the YAML file from disk and render it into the TraceConfig struct.
func (tc *TraceConfig) LoadConfig(r io.Reader) error {
	err := yaml.NewDecoder(r).Decode(tc)
	if err != nil {
		return err
	}
	return nil
}

// LoadConfigFromFile will load the configuration from file.
//

func LoadConfigFromFile(filename string) (*TraceConfig, error) {
	// cfg is a slice of strings unmarsalled from YAML
	cfg := new(TraceConfig)

	// Open passed in filename
	f, err := os.Open(filename)
	if err != nil {
		return &TraceConfig{}, err
	}
	defer f.Close()

	// Load from reader, validate
	err = cfg.LoadConfig(f)
	if err != nil {
		return &TraceConfig{}, err
	}

	validate = validator.New()
	err = validate.Struct(cfg)
	if err != nil {
		return &TraceConfig{}, err
	}
	err = cfg.CheckandSetValues()
	if err != nil {
		return &TraceConfig{}, err
	}

	return cfg, nil
}

func (tc *TraceConfig) CheckandSetValues() error {
	if tc.SchemaVersion != schemaVersion {
		return fmt.Errorf("unknown schema version %s", tc.SchemaVersion)
	}
	if tc.TraceConfigGlobal.MaxHops == 0 {
		tc.TraceConfigGlobal.MaxHops = defaultMaxHops
	}
	if tc.TraceConfigGlobal.NQueries == 0 {
		tc.TraceConfigGlobal.NQueries = defaultNumberQueries
	}
	if tc.TraceConfigGlobal.Interval == 0 {
		tc.TraceConfigGlobal.Interval = defaultInterval
	}
	if tc.TraceConfigGlobal.ParallelRequests == 0 {
		tc.TraceConfigGlobal.ParallelRequests = defaultParallelRequests
	}
	if tc.TraceConfigGlobal.Timeout == 0 {
		tc.TraceConfigGlobal.Timeout = defaultTimeout
	}
	return nil
}

// PrintEmptyConfiguration is used to generate an empty configuration to stdout
func PrintEmptyConfiguration() error {
	emptyConfig := TraceConfig{
		SchemaVersion: schemaVersion,
		TraceConfigDestinations: []string{
			"first-test-domain.org",
			"second-test-domain.org",
			"third-test-domain.net",
		},
		TraceConfigGlobal: TraceConfigGlobal{
			Protocol:         defaultProtocol,
			MaxHops:          defaultMaxHops,
			NQueries:         defaultNumberQueries,
			ParallelRequests: defaultParallelRequests,
			Timeout:          defaultTimeout,
			TraceRoutePort:   defaultTracePort,
			Interval:         defaultInterval,
		},
		TraceConfigOtel: TraceConfigOtel{
			Destination: "192.168.0.183",
			TLS:         false,
			Port:        4317,
			GRPC:        true,
		},
		TraceConfigHealthCheck: TraceConfigHealthCheck{
			Path:    "/_healthcheck",
			Enabled: true,
			Port:    8080,
		},
	}

	validate = validator.New()
	err := validate.Struct(emptyConfig)
	if err != nil {
		return err
	}

	yamlConfiguration, err := yaml.Marshal(&emptyConfig)
	if err != nil {
		return err
	}

	fmt.Printf("---\n%s\n\n", string(yamlConfiguration))

	return nil
}
