use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Duration;

/// Inference performance metrics for a single device.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InferenceMetrics {
    /// Total number of inferences performed.
    pub total_inferences: u64,
    /// Number of failed inferences.
    pub failed_inferences: u64,
    /// Average latency in microseconds.
    pub avg_latency_us: f64,
    /// P50 latency in microseconds.
    pub p50_latency_us: f64,
    /// P95 latency in microseconds.
    pub p95_latency_us: f64,
    /// P99 latency in microseconds.
    pub p99_latency_us: f64,
    /// Max latency in microseconds.
    pub max_latency_us: f64,
    /// Inferences per second (throughput).
    pub throughput_per_sec: f64,
    /// Memory usage in bytes.
    pub memory_usage_bytes: u64,
    /// Model load time in microseconds.
    pub model_load_time_us: u64,
    /// Uptime of the current model deployment.
    pub uptime_seconds: u64,
}

/// Latency tracker that computes percentiles from a ring buffer of measurements.
///
/// Uses a `VecDeque` for O(1) eviction of the oldest sample when the buffer is
/// full, instead of the O(n) `Vec::remove(0)` that was here previously.
#[derive(Debug, Clone)]
pub struct LatencyTracker {
    samples: VecDeque<f64>,
    max_samples: usize,
}

impl LatencyTracker {
    /// Create a new latency tracker that retains at most `max_samples` entries.
    #[must_use]
    pub fn new(max_samples: usize) -> Self {
        LatencyTracker {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    /// Record a latency sample from a [`Duration`].
    pub fn record(&mut self, duration: Duration) {
        self.record_us(duration.as_secs_f64() * 1_000_000.0);
    }

    /// Record a latency value already expressed in microseconds.
    pub fn record_us(&mut self, us: f64) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front(); // O(1)
        }
        self.samples.push_back(us);
    }

    /// Number of recorded samples currently held.
    #[must_use]
    pub fn count(&self) -> usize {
        self.samples.len()
    }

    /// Compute a percentile where `p` is in `0.0..=1.0`.
    ///
    /// Returns `0.0` when no samples have been recorded.
    #[must_use]
    pub fn percentile(&self, p: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f64> = self.samples.iter().copied().collect();
        sorted.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let idx = (p * (sorted.len() - 1) as f64).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    /// Arithmetic mean of all recorded latencies in microseconds.
    #[must_use]
    pub fn average(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().sum::<f64>() / self.samples.len() as f64
    }

    /// Maximum recorded latency in microseconds.
    #[must_use]
    pub fn max(&self) -> f64 {
        self.samples.iter().copied().fold(0.0_f64, f64::max)
    }

    /// Throughput in inferences per second, derived from the average latency.
    #[must_use]
    pub fn throughput(&self) -> f64 {
        let avg = self.average();
        if avg <= 0.0 {
            return 0.0;
        }
        1_000_000.0 / avg
    }

    /// Snapshot the current tracker state into an [`InferenceMetrics`].
    #[must_use]
    pub fn to_metrics(&self, total: u64, failed: u64, memory_bytes: u64) -> InferenceMetrics {
        InferenceMetrics {
            total_inferences: total,
            failed_inferences: failed,
            avg_latency_us: self.average(),
            p50_latency_us: self.percentile(0.50),
            p95_latency_us: self.percentile(0.95),
            p99_latency_us: self.percentile(0.99),
            max_latency_us: self.max(),
            throughput_per_sec: self.throughput(),
            memory_usage_bytes: memory_bytes,
            model_load_time_us: 0,
            uptime_seconds: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_latency_tracker_basic() {
        let mut tracker = LatencyTracker::new(1000);
        tracker.record(Duration::from_millis(10));
        tracker.record(Duration::from_millis(20));
        tracker.record(Duration::from_millis(30));

        assert_eq!(tracker.count(), 3);
        assert!((tracker.average() - 20_000.0).abs() < 1.0);
        assert!((tracker.percentile(0.5) - 20_000.0).abs() < 1.0);
    }

    #[test]
    fn test_latency_tracker_percentiles() {
        let mut tracker = LatencyTracker::new(1000);
        for i in 1..=100 {
            tracker.record_us(i as f64 * 100.0);
        }
        assert_eq!(tracker.count(), 100);

        let p50 = tracker.percentile(0.50);
        assert!(p50 > 4500.0 && p50 < 5500.0, "p50 = {p50}");

        let p99 = tracker.percentile(0.99);
        assert!(p99 > 9500.0 && p99 < 10100.0, "p99 = {p99}");
    }

    #[test]
    fn test_latency_tracker_overflow() {
        let mut tracker = LatencyTracker::new(5);
        for i in 0..10 {
            tracker.record_us(i as f64 * 100.0);
        }
        assert_eq!(tracker.count(), 5);
    }

    #[test]
    fn test_empty_tracker() {
        let tracker = LatencyTracker::new(100);
        assert_eq!(tracker.average(), 0.0);
        assert_eq!(tracker.percentile(0.5), 0.0);
        assert_eq!(tracker.throughput(), 0.0);
    }

    #[test]
    fn test_to_metrics() {
        let mut tracker = LatencyTracker::new(1000);
        for _ in 0..100 {
            tracker.record_us(1000.0);
        }
        let metrics = tracker.to_metrics(100, 2, 50_000_000);
        assert_eq!(metrics.total_inferences, 100);
        assert_eq!(metrics.failed_inferences, 2);
        assert!((metrics.avg_latency_us - 1000.0).abs() < 1.0);
        assert!((metrics.throughput_per_sec - 1000.0).abs() < 1.0);
        assert_eq!(metrics.memory_usage_bytes, 50_000_000);
    }
}
