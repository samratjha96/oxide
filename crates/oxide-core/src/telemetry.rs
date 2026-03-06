use serde::{Deserialize, Serialize};

use crate::device::DeviceId;
use crate::metrics::InferenceMetrics;
use crate::model::{ModelId, ModelVersion};

/// A telemetry report from a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryReport {
    /// Device that generated this report.
    pub device_id: DeviceId,
    /// Model currently running.
    pub model_id: ModelId,
    /// Model version currently running.
    pub model_version: ModelVersion,
    /// Inference performance metrics.
    pub metrics: InferenceMetrics,
    /// Timestamp of the report.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Device uptime in seconds.
    pub device_uptime_secs: u64,
    /// Free memory in bytes.
    pub free_memory_bytes: u64,
    /// CPU usage percentage (0.0 - 100.0).
    pub cpu_usage_percent: f64,
    /// Custom key-value metadata.
    pub metadata: std::collections::HashMap<String, String>,
}

impl TelemetryReport {
    /// Create a new telemetry report for the given device and model.
    pub fn new(
        device_id: DeviceId,
        model_id: ModelId,
        model_version: ModelVersion,
        metrics: InferenceMetrics,
    ) -> Self {
        TelemetryReport {
            device_id,
            model_id,
            model_version,
            metrics,
            timestamp: chrono::Utc::now(),
            device_uptime_secs: 0,
            free_memory_bytes: 0,
            cpu_usage_percent: 0.0,
            metadata: std::collections::HashMap::new(),
        }
    }
}

/// Queue for storing telemetry reports when offline.
#[derive(Debug)]
pub struct TelemetryQueue {
    reports: Vec<TelemetryReport>,
    max_size: usize,
}

impl TelemetryQueue {
    /// Create a new telemetry queue with the given maximum size.
    pub const fn new(max_size: usize) -> Self {
        TelemetryQueue {
            reports: Vec::new(),
            max_size,
        }
    }

    /// Enqueue a telemetry report. Returns false if the queue is full.
    pub fn enqueue(&mut self, report: TelemetryReport) -> bool {
        if self.reports.len() >= self.max_size {
            return false;
        }
        self.reports.push(report);
        true
    }

    /// Drain all pending reports.
    pub fn drain(&mut self) -> Vec<TelemetryReport> {
        std::mem::take(&mut self.reports)
    }

    /// Number of pending reports.
    pub const fn len(&self) -> usize {
        self.reports.len()
    }

    /// Check if queue is empty.
    pub const fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }

    /// Check if queue is full.
    pub const fn is_full(&self) -> bool {
        self.reports.len() >= self.max_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_report() -> TelemetryReport {
        TelemetryReport::new(
            DeviceId::from("pi-01"),
            ModelId::from("face-detection"),
            ModelVersion::from("v1.0.0"),
            InferenceMetrics::default(),
        )
    }

    #[test]
    fn test_telemetry_queue_basic() {
        let mut queue = TelemetryQueue::new(10);
        assert!(queue.is_empty());

        assert!(queue.enqueue(make_report()));
        assert_eq!(queue.len(), 1);
        assert!(!queue.is_empty());
    }

    #[test]
    fn test_telemetry_queue_overflow() {
        let mut queue = TelemetryQueue::new(2);
        assert!(queue.enqueue(make_report()));
        assert!(queue.enqueue(make_report()));
        assert!(!queue.enqueue(make_report())); // Full
        assert!(queue.is_full());
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_telemetry_queue_drain() {
        let mut queue = TelemetryQueue::new(10);
        queue.enqueue(make_report());
        queue.enqueue(make_report());

        let reports = queue.drain();
        assert_eq!(reports.len(), 2);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_telemetry_report_serialization() {
        let report = make_report();
        let json = serde_json::to_string(&report).unwrap();
        let deserialized: TelemetryReport = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.device_id.0, "pi-01");
    }
}
