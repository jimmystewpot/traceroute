//! Background service daemon and health-check monitoring facilities.
//! Designed and documented following Australian English conventions.

pub mod health;

pub use health::HealthState;

use crate::config::TraceConfig;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::time;

/// Runs the traceroute daemon delegating to [`run_daemon_with_shutdown`] using SIGINT (Ctrl+C).
pub async fn run_daemon(cfg: TraceConfig, state: Arc<HealthState>) -> anyhow::Result<()> {
    run_daemon_with_shutdown(cfg, state, async {
        let mut sigterm =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(sig) => sig,
                Err(e) => {
                    tracing::warn!("Failed to install SIGTERM handler: {}", e);
                    let _ = tokio::signal::ctrl_c().await;
                    return;
                }
            };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    })
    .await
}

/// Runs the traceroute daemon with a configurable shutdown signal.
///
/// Traces are dispatched periodically across all configured destinations.
/// When `shutdown_signal` completes, in-flight traces and the health server are
/// drained cleanly before returning.
pub async fn run_daemon_with_shutdown<F>(
    cfg: TraceConfig,
    state: Arc<HealthState>,
    shutdown_signal: F,
) -> anyhow::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Conditionally start Axum health server with graceful shutdown hooked to the watch channel.
    let health_handle = if cfg.healthcheck.enabled {
        let state_clone = Arc::clone(&state);
        let port = cfg.healthcheck.port;
        let path = cfg.healthcheck.path.clone();

        let shutdown_fut = async move {
            let _ = shutdown_rx.wait_for(|&s| s).await;
        };

        let server_fut = crate::service::health::run_health_server_with_shutdown(
            port,
            &path,
            state_clone,
            shutdown_fut,
        )
        .await?;

        tracing::info!("Health server listening on port {port}");

        let handle = tokio::spawn(async move {
            if let Err(e) = server_fut.await {
                tracing::error!("Health server terminated: {e}");
            }
        });
        Some(handle)
    } else {
        None
    };

    let span_cache: Arc<Mutex<HashMap<std::net::IpAddr, opentelemetry::trace::SpanContext>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(
        cfg.globals.parallel_requests.max(1) as usize,
    ));
    let mut interval = time::interval(cfg.globals.interval);
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    let mut active_batch: Option<tokio::task::JoinHandle<()>> = None;
    tokio::pin!(shutdown_signal);

    loop {
        tokio::select! {
            _ = &mut shutdown_signal => {
                tracing::info!("Shutdown signal received; draining in-flight traces...");
                let _ = shutdown_tx.send(true);
                if let Some(h) = health_handle {
                    if let Err(e) = h.await {
                        tracing::error!("Health server join error during shutdown: {e}");
                    }
                }
                if let Some(b) = active_batch {
                    if let Err(e) = b.await {
                        tracing::error!("In-flight batch join error during shutdown: {e}");
                    }
                }
                tracing::info!("All in-flight traces completed. Daemon shut down cleanly.");
                break;
            }
            _ = interval.tick() => {
                if let Some(ref prev) = active_batch {
                    if !prev.is_finished() {
                        tracing::warn!("Previous trace batch is still running; skipping tick.");
                        continue;
                    }
                }
                if let Some(prev) = active_batch.take() {
                    if let Err(e) = prev.await {
                        tracing::error!("Previous trace batch task error: {e}");
                    }
                }
                let batch_cfg = cfg.clone();
                let batch_state = Arc::clone(&state);
                let batch_sem = Arc::clone(&semaphore);
                let batch_span_cache = Arc::clone(&span_cache);
                active_batch = Some(tokio::spawn(async move {
                    dispatch_trace_batch(&batch_cfg, &batch_state, &batch_sem, &batch_span_cache).await;
                }));
            }
        }
    }
    Ok(())
}

/// Dispatches one round of concurrent traces across all configured destinations.
async fn dispatch_trace_batch(
    cfg: &TraceConfig,
    state: &Arc<HealthState>,
    semaphore: &Arc<tokio::sync::Semaphore>,
    span_cache: &Arc<Mutex<HashMap<std::net::IpAddr, opentelemetry::trace::SpanContext>>>,
) {
    let mut handles = Vec::new();
    for dest_str in &cfg.destinations {
        let dest_str = dest_str.clone();
        let state = Arc::clone(state);
        let sem = Arc::clone(semaphore);
        let span_cache = Arc::clone(span_cache);
        let linked_spans_enabled = cfg.opentelemetry.linked_spans;
        let protocol = cfg.globals.protocol.clone();
        let max_hops = cfg.globals.max_hops;
        let queries_per_hop = cfg.globals.number_queries;
        let timeout = cfg.globals.timeout;
        let dest_port = cfg.globals.source_port;
        let parallel = cfg.globals.parallel_requests;

        handles.push(tokio::spawn(async move {
            // Resolve destination
            let dns_start = std::time::Instant::now();
            let resolved = crate::cli::resolve_destination(&dest_str).await;
            let dns_latency_us = dns_start.elapsed().as_micros() as u64;
            state.record_dns_latency(dns_latency_us);

            let dest_ip = match resolved {
                Ok(ref ips) => match ips.first().copied() {
                    Some(ip) => ip,
                    None => {
                        tracing::warn!("DNS resolution returned no addresses for {}", dest_str);
                        state
                            .unsuccessful_traces
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        state
                            .total_traces
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                },
                Err(e) => {
                    tracing::warn!("DNS resolution failed for {}: {}", dest_str, e);
                    state
                        .unsuccessful_traces
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    state
                        .total_traces
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return;
                }
            };

            state
                .total_traces
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            let span_links = if linked_spans_enabled {
                let cache_guard = match span_cache.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                cache_guard
                    .get(&dest_ip)
                    .map(|prev_ctx| {
                        vec![opentelemetry::trace::Link::new(
                            prev_ctx.clone(),
                            Vec::new(),
                            0,
                        )]
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            let (reached, returned_ctx) = match protocol.as_str() {
                "tcp" => {
                    use crate::engine::tcp::{
                        run_tcp_traceroute_with_span_context, TcpTracerouteConfig,
                    };
                    let probe_cfg = TcpTracerouteConfig {
                        destination: dest_ip,
                        destination_name: Some(dest_str.clone()),
                        max_hops,
                        queries_per_hop,
                        parallel_requests: parallel,
                        timeout,
                        dest_port,
                        span_links,
                        probe_limiter: Some(Arc::clone(&sem)),
                    };
                    match run_tcp_traceroute_with_span_context(probe_cfg).await {
                        Ok((hops, span_ctx)) => {
                            let reached = hops.values().any(|h| {
                                h.iter()
                                    .any(|hop| hop.success && hop.address == Some(dest_ip))
                            });
                            (reached, Some(span_ctx))
                        }
                        Err(e) => {
                            tracing::warn!("TCP traceroute error for {}: {}", dest_str, e);
                            (false, None)
                        }
                    }
                }
                _ => {
                    use crate::engine::udp::{
                        run_udp_traceroute_with_span_context, UdpTracerouteConfig,
                    };
                    let probe_cfg = UdpTracerouteConfig {
                        destination: dest_ip,
                        destination_name: Some(dest_str.clone()),
                        max_hops,
                        queries_per_hop,
                        parallel_requests: parallel,
                        timeout,
                        dest_port,
                        span_links,
                        probe_limiter: Some(Arc::clone(&sem)),
                    };
                    match run_udp_traceroute_with_span_context(probe_cfg).await {
                        Ok((hops, span_ctx)) => {
                            let reached = hops.values().any(|h| {
                                h.iter()
                                    .any(|hop| hop.success && hop.address == Some(dest_ip))
                            });
                            (reached, Some(span_ctx))
                        }
                        Err(e) => {
                            tracing::warn!("UDP traceroute error for {}: {}", dest_str, e);
                            (false, None)
                        }
                    }
                }
            };

            if linked_spans_enabled {
                if let Some(ctx) = returned_ctx {
                    let mut cache_guard = match span_cache.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    cache_guard.insert(dest_ip, ctx);
                }
            }

            if reached {
                state
                    .successful_traces
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                state
                    .unsuccessful_traces
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }));
    }
    for h in handles {
        let _ = h.await;
    }
}
