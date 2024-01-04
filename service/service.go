package service

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"os/signal"
	"sync"
	"time"

	"github.com/jimmystewpot/traceroute/config"
)

var (
	healthCheckService http.Server
)

type Service struct {
	Hostname string
	Config   config.TraceConfig
	close    chan struct{}
}

type HealthCheck struct {
	Status      string             `json:"status"`
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

func New(cfg config.TraceConfig) *Service {
	return &Service{
		Config: cfg,
		close:  make(chan struct{}),
	}
}

func (svc *Service) Start() error {
	hc := HealthCheck{
		Details: HealthCheckDetails{},
		close:   make(chan struct{}),
	}
	if svc.Config.TraceConfigHealthCheck.Enabled {
		go hc.RunHealthCheckSvc(svc.Config.TraceConfigHealthCheck)
		go func() {
			sigint := make(chan os.Signal, 1)
			signal.Notify(sigint, os.Interrupt)
			<-sigint

			// We received an interrupt signal, shut down.
			if err := healthCheckService.Shutdown(context.Background()); err != nil {
				// Error from closing listeners, or context timeout:
				log.Printf("HTTP server Shutdown: %v", err)
			}
			close(hc.close)
		}()
	}
	// set the interval at which the traceroutes are executed.
	ticker := time.NewTicker(svc.Config.TraceConfigGlobal.Interval)
	for {
		select {
		case <-ticker.C:
			for i := 0; i < len(svc.Config.TraceConfigDestinations); i++ {
				// xid and cid need to be set on a per-traceroute basis.
			}
		}
	}
	return nil
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
	return
}

func (health *HealthCheck) invalid(w http.ResponseWriter, req *http.Request) {
	http.Error(w, "invalid request", http.StatusBadRequest)
	return
}
