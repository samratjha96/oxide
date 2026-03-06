use thiserror::Error;

/// Core error type for the Oxide runtime.
///
/// Prefer the specific variants (e.g. `ModelNotFound`, `Encryption`) over the
/// generic ones (`Internal`) so callers can pattern-match on failure modes.
#[derive(Error, Debug)]
pub enum OxideError {
    #[error("Model error: {0}")]
    Model(String),

    #[error("Inference error: {0}")]
    Inference(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Unsupported model format: {0}")]
    UnsupportedFormat(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Device error: {0}")]
    Device(String),

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Fleet error: {0}")]
    Fleet(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Security error: {0}")]
    Security(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Decryption error: {0}")]
    Decryption(String),

    #[error("Update error: {0}")]
    Update(String),

    #[error("Rollback error: {0}")]
    Rollback(String),

    #[error("Health check failed: {0}")]
    HealthCheck(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Convenience Result type for Oxide operations.
pub type Result<T> = std::result::Result<T, OxideError>;
