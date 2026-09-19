//! Tells a starved async runtime apart from an exhausted connection pool.
//!
//! Both look the same from inside a request: sqlx reports a slow connection
//! acquire and a slow statement, because its timers run on the same runtime
//! and include the wait for a connection. This monitor samples the two causes
//! directly, once a second, without taking a connection.

use std::time::Duration;

use metrics::gauge;
use scryer_infrastructure_sql::runtime::{PoolUsage, StoreDatastore};
use tokio_util::sync::CancellationToken;

const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
/// A one second tick that arrives this late means no worker thread was free to
/// poll it: blocking work is running on the async runtime.
const RUNTIME_STALL_WARN_LAG: Duration = Duration::from_secs(1);
/// A pool that is momentarily full is normal under a burst of reads.
const POOL_SATURATION_WARN_SAMPLES: u32 = 3;
const WARN_REPEAT_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, PartialEq, Eq)]
enum HealthWarning {
    RuntimeStalled { lag: Duration },
    PoolSaturated { samples: u32 },
}

#[derive(Default)]
struct HealthWatch {
    saturated_samples: u32,
    since_runtime_warning: Option<Duration>,
    since_pool_warning: Option<Duration>,
}

impl HealthWatch {
    /// `elapsed` is the time since the previous sample.
    fn observe(&mut self, elapsed: Duration, usage: PoolUsage) -> Vec<HealthWarning> {
        let mut warnings = Vec::new();
        for since in [
            &mut self.since_runtime_warning,
            &mut self.since_pool_warning,
        ] {
            if let Some(waited) = since {
                *waited += elapsed;
                if *waited >= WARN_REPEAT_INTERVAL {
                    *since = None;
                }
            }
        }

        let lag = elapsed.saturating_sub(SAMPLE_INTERVAL);
        if lag >= RUNTIME_STALL_WARN_LAG && self.since_runtime_warning.is_none() {
            self.since_runtime_warning = Some(Duration::ZERO);
            warnings.push(HealthWarning::RuntimeStalled { lag });
        }

        if usage.is_saturated() {
            self.saturated_samples += 1;
        } else {
            self.saturated_samples = 0;
        }
        if self.saturated_samples >= POOL_SATURATION_WARN_SAMPLES
            && self.since_pool_warning.is_none()
        {
            self.since_pool_warning = Some(Duration::ZERO);
            warnings.push(HealthWarning::PoolSaturated {
                samples: self.saturated_samples,
            });
        }
        warnings
    }
}

pub(crate) fn describe_runtime_health_metrics() {
    metrics::describe_gauge!(
        "scryer_runtime_tick_lag_seconds",
        metrics::Unit::Seconds,
        "How late the last one second health tick was polled. Sustained lag means blocking work is starving the async runtime, which inflates every other latency the process reports."
    );
    metrics::describe_gauge!(
        "scryer_datastore_pool_open_connections",
        "Datastore connections currently open, idle or checked out."
    );
    metrics::describe_gauge!(
        "scryer_datastore_pool_idle_connections",
        "Open datastore connections nobody is using. Zero while open connections sit at the maximum means the next query waits."
    );
    metrics::describe_gauge!(
        "scryer_datastore_writer_gate_held",
        "1 while a write holds the sqlite single-writer gate, otherwise 0."
    );
}

pub(crate) async fn start_runtime_health_monitor(
    datastore: StoreDatastore,
    token: CancellationToken,
) {
    let mut ticks = tokio::time::interval(SAMPLE_INTERVAL);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticks.tick().await;
    let mut last_sample = tokio::time::Instant::now();
    let mut watch = HealthWatch::default();

    loop {
        tokio::select! {
            _ = token.cancelled() => return,
            _ = ticks.tick() => {}
        }
        let now = tokio::time::Instant::now();
        let elapsed = now.duration_since(last_sample);
        last_sample = now;
        let usage = datastore.pool_usage();

        gauge!("scryer_runtime_tick_lag_seconds")
            .set(elapsed.saturating_sub(SAMPLE_INTERVAL).as_secs_f64());
        gauge!("scryer_datastore_pool_open_connections").set(f64::from(usage.open_connections));
        gauge!("scryer_datastore_pool_idle_connections").set(usage.idle_connections as f64);
        gauge!("scryer_datastore_writer_gate_held").set(if usage.writer_gate_held {
            1.0
        } else {
            0.0
        });

        for warning in watch.observe(elapsed, usage) {
            match warning {
                HealthWarning::RuntimeStalled { lag } => tracing::warn!(
                    lag_ms = lag.as_millis() as u64,
                    pool_open = usage.open_connections,
                    pool_idle = usage.idle_connections,
                    pool_max = usage.max_connections,
                    writer_gate_held = usage.writer_gate_held,
                    "async runtime stalled: a one second health tick ran late, so slow-query and slow-acquire timings around it are inflated"
                ),
                HealthWarning::PoolSaturated { samples } => tracing::warn!(
                    saturated_seconds = samples,
                    pool_max = usage.max_connections,
                    writer_gate_held = usage.writer_gate_held,
                    "datastore connection pool saturated: every connection is checked out and queries are waiting for one"
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(open: u32, idle: usize) -> PoolUsage {
        PoolUsage {
            max_connections: 16,
            open_connections: open,
            idle_connections: idle,
            writer_gate_held: false,
        }
    }

    #[test]
    fn an_on_time_tick_with_a_free_connection_is_quiet() {
        let mut watch = HealthWatch::default();
        assert!(watch.observe(SAMPLE_INTERVAL, usage(16, 1)).is_empty());
        assert!(
            watch
                .observe(Duration::from_millis(1_900), usage(4, 4))
                .is_empty()
        );
    }

    #[test]
    fn a_late_tick_reports_the_runtime_once_per_repeat_interval() {
        let mut watch = HealthWatch::default();
        assert_eq!(
            watch.observe(Duration::from_secs(15), usage(4, 4)),
            vec![HealthWarning::RuntimeStalled {
                lag: Duration::from_secs(14)
            }]
        );
        assert!(
            watch
                .observe(Duration::from_secs(5), usage(4, 4))
                .is_empty()
        );
        assert_eq!(
            watch.observe(Duration::from_secs(40), usage(4, 4)),
            vec![HealthWarning::RuntimeStalled {
                lag: Duration::from_secs(39)
            }]
        );
    }

    #[test]
    fn only_a_sustained_full_pool_is_reported() {
        let mut watch = HealthWatch::default();
        assert!(watch.observe(SAMPLE_INTERVAL, usage(16, 0)).is_empty());
        assert!(watch.observe(SAMPLE_INTERVAL, usage(16, 0)).is_empty());
        assert!(watch.observe(SAMPLE_INTERVAL, usage(16, 2)).is_empty());
        assert!(watch.observe(SAMPLE_INTERVAL, usage(16, 0)).is_empty());
        assert!(watch.observe(SAMPLE_INTERVAL, usage(16, 0)).is_empty());
        assert_eq!(
            watch.observe(SAMPLE_INTERVAL, usage(16, 0)),
            vec![HealthWarning::PoolSaturated { samples: 3 }]
        );
        assert!(watch.observe(SAMPLE_INTERVAL, usage(16, 0)).is_empty());
    }

    #[test]
    fn a_pool_below_its_maximum_is_never_saturated() {
        let mut watch = HealthWatch::default();
        for _ in 0..5 {
            assert!(watch.observe(SAMPLE_INTERVAL, usage(8, 0)).is_empty());
        }
    }
}
