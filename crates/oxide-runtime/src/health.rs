//! Health checking for runtime and models.

use oxide_core::error::Result;
use oxide_core::model::ModelId;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::engine::InferenceEngine;

/// Health check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Overall health status.
    pub healthy: bool,
    /// Individual check results.
    pub checks: Vec<CheckResult>,
    /// Time taken for all checks in microseconds.
    pub check_duration_us: f64,
}

/// Result of a single health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Name of the check.
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Optional detail message.
    pub message: Option<String>,
}

/// Configurable health checker.
pub struct HealthChecker {
    /// Maximum allowed average latency in microseconds.
    pub max_avg_latency_us: f64,
    /// Maximum error rate (0.0 to 1.0).
    pub max_error_rate: f64,
    /// Minimum required inferences before checking error rate.
    pub min_inferences_for_rate: u64,
}

impl Default for HealthChecker {
    fn default() -> Self {
        HealthChecker {
            max_avg_latency_us: 50_000.0, // 50ms
            max_error_rate: 0.05,          // 5%
            min_inferences_for_rate: 10,
        }
    }
}

impl HealthChecker {
    /// Run health checks against a model in the inference engine.
    pub fn check(&self, engine: &InferenceEngine, model_id: &ModelId) -> Result<HealthStatus> {
        let start = Instant::now();
        let mut checks = Vec::new();

        // Check 1: Model is loaded
        let model_loaded = engine.is_loaded(model_id);
        checks.push(CheckResult {
            name: "model_loaded".to_string(),
            passed: model_loaded,
            message: if model_loaded {
                None
            } else {
                Some(format!("Model '{}' is not loaded", model_id))
            },
        });

        if !model_loaded {
            return Ok(HealthStatus {
                healthy: false,
                checks,
                check_duration_us: start.elapsed().as_secs_f64() * 1_000_000.0,
            });
        }

        // Check 2: Metrics within bounds
        if let Ok(metrics) = engine.get_metrics(model_id) {
            // Latency check
            let latency_ok = metrics.avg_latency_us <= self.max_avg_latency_us
                || metrics.total_inferences == 0;
            checks.push(CheckResult {
                name: "latency".to_string(),
                passed: latency_ok,
                message: if latency_ok {
                    Some(format!("avg: {:.2}us", metrics.avg_latency_us))
                } else {
                    Some(format!(
                        "avg latency {:.2}us exceeds max {:.2}us",
                        metrics.avg_latency_us, self.max_avg_latency_us
                    ))
                },
            });

            // Error rate check
            if metrics.total_inferences >= self.min_inferences_for_rate {
                let error_rate =
                    metrics.failed_inferences as f64 / metrics.total_inferences as f64;
                let rate_ok = error_rate <= self.max_error_rate;
                checks.push(CheckResult {
                    name: "error_rate".to_string(),
                    passed: rate_ok,
                    message: if rate_ok {
                        Some(format!("{:.2}%", error_rate * 100.0))
                    } else {
                        Some(format!(
                            "error rate {:.2}% exceeds max {:.2}%",
                            error_rate * 100.0,
                            self.max_error_rate * 100.0
                        ))
                    },
                });
            }
        }

        let healthy = checks.iter().all(|c| c.passed);
        let duration = start.elapsed().as_secs_f64() * 1_000_000.0;

        Ok(HealthStatus {
            healthy,
            checks,
            check_duration_us: duration,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check_no_model() {
        let engine = InferenceEngine::new(1);
        let checker = HealthChecker::default();
        let result = checker
            .check(&engine, &ModelId::from("nonexistent"))
            .unwrap();
        assert!(!result.healthy);
        assert_eq!(result.checks.len(), 1);
        assert!(!result.checks[0].passed);
    }

    #[test]
    fn test_health_checker_defaults() {
        let checker = HealthChecker::default();
        assert_eq!(checker.max_avg_latency_us, 50_000.0);
        assert_eq!(checker.max_error_rate, 0.05);
    }
}
