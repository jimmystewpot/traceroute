package service

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/signal"
	"sync"
	"syscall"
	"time"

	"github.com/jimmystewpot/traceroute/config"
	"github.com/jimmystewpot/traceroute/trace"
	"go.uber.org/zap"
)

const (
	ServiceName string = "opentelemetry-traceroute"
)

var (
	healthCheckService http.Server
	logger             *zap.Logger
)

type Service struct {
	Hostname string
	Config   config.TraceConfig
	close    chan struct{}
}

type HealthCheck struct {
	Status      string             `json:"status"`
	Service     string             `json:"service-name"`
	Hostname    string             `json:"hostname"`
	CurrentTime string             `json:"current-time"`
	Details     HealthCheckDetails `json:"details"`
	mutex       sync.Mutex         // for locking when writing stats
	close       chan struct{}      // for closing down the healthcheck endpoint cleanly
}

type HealthCheckDetails struct {
	SuccessfulTraces   uint64          `json:"successful-traces"`
	UnsuccessfulTraces uint64          `json:"unsuccessful-traces"`
	TotalTraces        uint64          `json:"total-traces"`
	DNSLatency         []time.Duration `json:"dns-latency"`
}
type ServiceCLI struct {
	ConfigFile     string `help:"Load a YAML configuration file" env:"TRACE_CFGFILE" required:"ValidateConfig"`
	ValidateConfig bool   `cmd:"" help:"Validate the configuration file format is correct" name:"validate"`
}

func (scli *ServiceCLI) Run() error {
	logger, _ = zap.NewProduction(zap.AddCaller())
	if scli.ValidateConfig {
		_, err := config.LoadConfigFromFile(scli.ConfigFile)
		if err != nil {
			logger.Warn("configuration failed to validate",
				zap.Error(err),
			)
			return fmt.Errorf("configuration failed to validate: %s", err)
		}
		return nil
	}
	cfg, err := config.LoadConfigFromFile(scli.ConfigFile)
	if err != nil {
		logger.Fatal("configuration failed to load",
			zap.Error(err),
		)
	}
	svc, err := New(cfg)
	if err != nil {
		logger.Warn("service failed to instantiate",
			zap.Error(err),
		)
	}
	svc.Start()

	return nil
}

func New(cfg *config.TraceConfig) (*Service, error) {
	hostname, err := os.Hostname()
	if err != nil {
		return &Service{}, err
	}

	return &Service{
		Config:   *cfg,
		Hostname: hostname,
		close:    make(chan struct{}),
	}, nil
}

func (svc *Service) Start() error {
	svc.LogStart()
	hc := HealthCheck{
		Details: HealthCheckDetails{},
		close:   make(chan struct{}),
	}
	if svc.Config.TraceConfigHealthCheck.Enabled {
		go hc.RunHealthCheckSvc(svc.Config.TraceConfigHealthCheck)
		go func() {
			sigint := make(chan os.Signal, 1)
			signal.Notify(sigint, syscall.SIGINT, syscall.SIGTERM)
			<-sigint

			//logger.Fatal("Shutting Down")
			// We received an interrupt signal, shut down.
			ctx, cancel := context.WithDeadline(context.Background(), time.Now().Add(60*time.Second))
			defer cancel()
			if err := healthCheckService.Shutdown(ctx); err != nil {
				// Error from closing listeners, or context timeout:
				logger.Warn("http server shutdown",
					zap.Error(err))
			}
			close(hc.close)
		}()
	}
	// set the interval at which the traceroutes are executed.
	ticker := time.NewTicker(svc.Config.TraceConfigGlobal.Interval)
	globalCfg := trace.TraceCLI{
		MaxHops:                  svc.Config.TraceConfigGlobal.MaxHops,
		NQueries:                 svc.Config.TraceConfigGlobal.NQueries,
		ParallelRequests:         svc.Config.TraceConfigGlobal.ParallelRequests,
		Timeout:                  svc.Config.TraceConfigGlobal.Timeout,
		TraceRoutePort:           svc.Config.TraceConfigGlobal.TraceRoutePort,
		OpenTelemetryDestination: svc.Config.TraceConfigOtel.Destination,
		OpenTelemetryTLS:         svc.Config.TraceConfigOtel.TLS,
		OpenTelemetryGRPC:        svc.Config.TraceConfigOtel.GRPC,
		OpenTelemetryPort:        svc.Config.TraceConfigOtel.Port,
		Hostname:                 svc.Hostname,
	}
	for {
		select {
		case <-ticker.C:
			for i := 0; i < len(svc.Config.TraceConfigDestinations); i++ {
				// copies the globalCfg to only update the variabels that change with
				// each traceroute.
				t := globalCfg
				t.Destination = svc.Config.TraceConfigDestinations[i]
				s := time.Now()
				if svc.Config.TraceConfigGlobal.Protocol == "udp" {
					err := t.UDP()
					if err != nil {
						logger.Warn("udp traceroute",
							zap.Error(err),
						)
					}
				}
				if svc.Config.TraceConfigGlobal.Protocol == "tcp" {
					err := t.TCP()
					if err != nil {
						logger.Warn("tcp traceroute",
							zap.Error(err),
						)
					}
				}
				logger.Info("tracing",
					zap.String("service_name", ServiceName),
					zap.String("destination", t.Destination),
					zap.Duration("duration", time.Since(s)),
				)
			}

			logger.Sync()
		case <-svc.close:
			logger.Warn("svc.close",
				zap.String("msg", "closing down gracefully"),
			)

		}
	}
	return nil
}

func (svc *Service) LogStart() {
	logger.Info("starting",
		zap.String("service_name", ServiceName),
		zap.Dict("configuration",
			zap.String("schema-version", svc.Config.SchemaVersion),
			zap.Strings("destinations", svc.Config.TraceConfigDestinations),
			zap.Dict("globals",
				zap.Uint16("max-hops", svc.Config.TraceConfigGlobal.MaxHops),
				zap.Uint16("number-queries", svc.Config.TraceConfigGlobal.NQueries),
				zap.Uint16("parallel-requests", svc.Config.TraceConfigGlobal.ParallelRequests),
				zap.String("protocol", svc.Config.TraceConfigGlobal.Protocol),
				zap.String("timeout", svc.Config.TraceConfigGlobal.Timeout.String()),
			),
			zap.Dict("opentelemetry",
				zap.String("destination", svc.Config.TraceConfigOtel.Destination),
				zap.Bool("tls", svc.Config.TraceConfigOtel.TLS),
				zap.Bool("grpc", svc.Config.TraceConfigOtel.GRPC),
				zap.Int("port", svc.Config.TraceConfigOtel.Port),
			),
		),
	)
}

// RunHealthCheckSvc will launch the background process to serve health check requests.
func (health *HealthCheck) RunHealthCheckSvc(cfg config.TraceConfigHealthCheck) error {
	// instantiate the http port
	healthCheckService.Addr = fmt.Sprintf(":%d", cfg.Port)

	// handle the http healthcheck Get request
	http.HandleFunc(cfg.Path, health.Get)
	// fallback for any other request to raise an error.
	http.HandleFunc("/", health.invalid)

	if err := healthCheckService.ListenAndServe(); err != http.ErrServerClosed {
		// Error starting or closing listener:
		return fmt.Errorf("HTTP server ListenAndServe: %v", err)
	}

	// block until healthcheck is being shutdown by the background goroutine sigint.
	<-health.close
	return nil
}

func (health *HealthCheck) Get(w http.ResponseWriter, req *http.Request) {
	health.mutex.Lock()
	defer health.mutex.Unlock()
	health.CurrentTime = time.Now().Format(time.RFC3339)

	// convert the struct into json.
	response, err := json.Marshal(health)
	if err != nil {
		health.mutex.Unlock()
		http.Error(w, "invalid request", http.StatusBadRequest)
	}
	// write headers and body to response.
	w.Header().Add("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	io.WriteString(w, string(response))
}

// invalid returns an error for any paths except for the HealthCheck.
func (health *HealthCheck) invalid(w http.ResponseWriter, req *http.Request) {
	logger.Info("bad request",
		zap.String("path", req.RequestURI),
		zap.String("src-ip", req.RemoteAddr),
		zap.String("user-agent", req.UserAgent()),
		zap.String("method", req.Method),
	)
	http.Error(w, "invalid request", http.StatusBadRequest)
}
