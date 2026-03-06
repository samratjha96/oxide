//! Filesystem-backed model store for the control plane.
//!
//! Stores model bytes on disk so devices can download them.
//! Separate from oxide-runtime's ModelStore — this one doesn't parse models,
//! just stores and serves bytes with integrity verification.

use oxide_core::error::{OxideError, Result};
use oxide_core::model::{ModelId, ModelVersion};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::info;

/// A stored model entry with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredModelEntry {
    pub model_id: ModelId,
    pub version: ModelVersion,
    pub sha256: String,
    pub size_bytes: u64,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
}

/// Filesystem-backed model store for the control plane.
pub struct ControlPlaneModelStore {
    root: PathBuf,
    index: HashMap<ModelId, Vec<StoredModelEntry>>,
}

impl ControlPlaneModelStore {
    /// Open or create a model store at the given directory.
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let index_path = root.join("model_index.json");

        let index: HashMap<ModelId, Vec<StoredModelEntry>> = if index_path.exists() {
            let content = std::fs::read_to_string(&index_path)?;
            serde_json::from_str(&content).map_err(|e| {
                OxideError::Serialization(format!("Failed to parse model index: {}", e))
            })?
        } else {
            HashMap::new()
        };

        info!(
            "Opened control plane model store at {} ({} models)",
            root.display(),
            index.len()
        );

        let store = ControlPlaneModelStore {
            root: root.to_path_buf(),
            index,
        };
        store.save_index()?;
        Ok(store)
    }

    /// Store model bytes. Returns the stored entry.
    ///
    /// # Errors
    /// Returns an error if the `model_id` or `version` contain path-traversal
    /// characters, or if writing fails.
    pub fn store(
        &mut self,
        model_id: &ModelId,
        version: &ModelVersion,
        data: &[u8],
    ) -> Result<StoredModelEntry> {
        Self::validate_path_component(&model_id.0)?;
        Self::validate_path_component(&version.0)?;

        let sha256 = Self::sha256_hex(data);
        let size_bytes = data.len() as u64;

        // Create directory structure: <root>/<model_id>/
        let model_dir = self.root.join(&model_id.0);
        std::fs::create_dir_all(&model_dir)?;

        // Write file: <root>/<model_id>/<version>.onnx
        let file_path = model_dir.join(format!("{}.onnx", version.0));
        std::fs::write(&file_path, data)?;

        let entry = StoredModelEntry {
            model_id: model_id.clone(),
            version: version.clone(),
            sha256,
            size_bytes,
            uploaded_at: chrono::Utc::now(),
        };

        // Update index (replace if same version exists)
        let entries = self.index.entry(model_id.clone()).or_default();
        entries.retain(|e| &e.version != version);
        entries.push(entry.clone());

        self.save_index()?;

        info!(
            "Stored model '{}' version '{}' ({} bytes, sha256={}...)",
            model_id,
            version,
            size_bytes,
            &entry.sha256[..8]
        );

        Ok(entry)
    }

    /// Get model bytes.
    pub fn get_bytes(&self, model_id: &ModelId, version: &ModelVersion) -> Result<Vec<u8>> {
        Self::validate_path_component(&model_id.0)?;
        Self::validate_path_component(&version.0)?;

        let file_path = self.root.join(&model_id.0).join(format!("{}.onnx", version.0));
        if !file_path.exists() {
            return Err(OxideError::ModelNotFound(format!(
                "{}@{}", model_id, version
            )));
        }
        Ok(std::fs::read(&file_path)?)
    }

    /// Get metadata for a stored model.
    pub fn get_meta(
        &self,
        model_id: &ModelId,
        version: &ModelVersion,
    ) -> Result<&StoredModelEntry> {
        let entries = self.index.get(model_id).ok_or_else(|| {
            OxideError::ModelNotFound(model_id.to_string())
        })?;

        entries
            .iter()
            .find(|e| &e.version == version)
            .ok_or_else(|| {
                OxideError::ModelNotFound(format!("{}@{}", model_id, version))
            })
    }

    /// List all versions of a model.
    pub fn list_versions(&self, model_id: &ModelId) -> Result<&Vec<StoredModelEntry>> {
        self.index.get(model_id).ok_or_else(|| {
            OxideError::ModelNotFound(model_id.to_string())
        })
    }

    /// List all models.
    pub fn list_all(&self) -> Vec<&StoredModelEntry> {
        self.index.values().flatten().collect()
    }

    /// Reject path components that could escape the store directory.
    fn validate_path_component(s: &str) -> Result<()> {
        if s.is_empty()
            || s.contains('/')
            || s.contains('\\')
            || s.contains("..")
            || s.contains('\0')
        {
            return Err(OxideError::Security(format!(
                "invalid identifier (path traversal rejected): {s:?}"
            )));
        }
        Ok(())
    }

    fn save_index(&self) -> Result<()> {
        let path = self.root.join("model_index.json");
        let content = serde_json::to_string_pretty(&self.index)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    fn sha256_hex(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_store_and_retrieve() {
        let dir = TempDir::new().unwrap();
        let mut store = ControlPlaneModelStore::open(dir.path()).unwrap();

        let data = b"fake model bytes";
        let entry = store
            .store(
                &ModelId::from("test-model"),
                &ModelVersion::from("v1.0.0"),
                data,
            )
            .unwrap();

        assert_eq!(entry.size_bytes, data.len() as u64);
        assert!(!entry.sha256.is_empty());

        let retrieved = store
            .get_bytes(&ModelId::from("test-model"), &ModelVersion::from("v1.0.0"))
            .unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn test_get_meta() {
        let dir = TempDir::new().unwrap();
        let mut store = ControlPlaneModelStore::open(dir.path()).unwrap();

        let data = b"model data";
        store
            .store(
                &ModelId::from("model"),
                &ModelVersion::from("v1.0.0"),
                data,
            )
            .unwrap();

        let meta = store
            .get_meta(&ModelId::from("model"), &ModelVersion::from("v1.0.0"))
            .unwrap();
        assert_eq!(meta.size_bytes, data.len() as u64);
    }

    #[test]
    fn test_multiple_versions() {
        let dir = TempDir::new().unwrap();
        let mut store = ControlPlaneModelStore::open(dir.path()).unwrap();

        store
            .store(
                &ModelId::from("model"),
                &ModelVersion::from("v1.0.0"),
                b"v1",
            )
            .unwrap();
        store
            .store(
                &ModelId::from("model"),
                &ModelVersion::from("v2.0.0"),
                b"v2",
            )
            .unwrap();

        let versions = store.list_versions(&ModelId::from("model")).unwrap();
        assert_eq!(versions.len(), 2);
    }

    #[test]
    fn test_persistence() {
        let dir = TempDir::new().unwrap();

        {
            let mut store = ControlPlaneModelStore::open(dir.path()).unwrap();
            store
                .store(
                    &ModelId::from("model"),
                    &ModelVersion::from("v1.0.0"),
                    b"data",
                )
                .unwrap();
        }

        {
            let store = ControlPlaneModelStore::open(dir.path()).unwrap();
            let retrieved = store
                .get_bytes(&ModelId::from("model"), &ModelVersion::from("v1.0.0"))
                .unwrap();
            assert_eq!(retrieved, b"data");
        }
    }

    #[test]
    fn test_not_found() {
        let dir = TempDir::new().unwrap();
        let store = ControlPlaneModelStore::open(dir.path()).unwrap();
        assert!(store
            .get_bytes(&ModelId::from("nope"), &ModelVersion::from("v1"))
            .is_err());
    }

    #[test]
    fn test_path_traversal_rejected() {
        let dir = TempDir::new().unwrap();
        let mut store = ControlPlaneModelStore::open(dir.path()).unwrap();

        let bad_ids = ["../etc", "foo/bar", "a\\b", "..", ""];
        for bad in &bad_ids {
            let result = store.store(
                &ModelId::from(*bad),
                &ModelVersion::from("v1"),
                b"data",
            );
            assert!(result.is_err(), "should reject model_id={bad:?}");
        }

        // Also reject bad versions
        let result = store.store(
            &ModelId::from("ok"),
            &ModelVersion::from("../v1"),
            b"data",
        );
        assert!(result.is_err(), "should reject version with ..");
    }
}
