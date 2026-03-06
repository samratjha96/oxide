//! Tensor manifest — per-tensor SHA-256 hashes.
//!
//! The manifest is what the agent sends in its heartbeat so the control
//! plane knows which tensors the device already has.

use crate::format::TensorInfo;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A manifest of tensor hashes for a model file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TensorManifest {
    pub entries: Vec<ManifestEntry>,
}

/// One entry in the tensor manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub name: String,
    pub sha256: [u8; 32],
    pub size: usize,
}

impl TensorManifest {
    /// Build a manifest from extracted tensors.
    pub fn from_tensors(tensors: &[TensorInfo]) -> Self {
        let entries = tensors
            .iter()
            .map(|t| {
                let mut hasher = Sha256::new();
                hasher.update(&t.data);
                ManifestEntry {
                    name: t.name.clone(),
                    sha256: hasher.finalize().into(),
                    size: t.data.len(),
                }
            })
            .collect();
        Self { entries }
    }

    /// Compact string representation for HTTP headers.
    /// Format: `name1=hex1,name2=hex2,...`
    pub fn to_header_value(&self) -> String {
        self.entries
            .iter()
            .map(|e| format!("{}={}", e.name, hex::encode(e.sha256)))
            .collect::<Vec<_>>()
            .join(",")
    }
}

mod hex {
    /// Encode bytes as lowercase hex string.
    pub fn encode(bytes: [u8; 32]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
