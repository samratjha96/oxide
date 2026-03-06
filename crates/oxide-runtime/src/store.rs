//! Model store: manages model files on disk with versioning and rollback.

use oxide_core::error::{OxideError, Result};
use oxide_core::model::{ModelFormat, ModelId, ModelVersion};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::info;

/// Manages model files on disk with versioning support.
pub struct ModelStore {
    /// Root directory for the model store.
    root: PathBuf,
    /// In-memory index of available models.
    index: HashMap<ModelId, Vec<ModelEntry>>,
}

/// An entry in the model store index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Model ID.
    pub id: ModelId,
    /// Version of this model.
    pub version: ModelVersion,
    /// File format.
    pub format: ModelFormat,
    /// Path to the model file (relative to store root).
    pub path: PathBuf,
    /// Size in bytes.
    pub size_bytes: u64,
    /// SHA-256 hash of the model file.
    pub sha256: String,
    /// When the model was added to the store.
    pub added_at: chrono::DateTime<chrono::Utc>,
}

impl ModelStore {
    /// Create or open a model store at the given directory.
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let index_path = root.join("index.json");

        let index: HashMap<ModelId, Vec<ModelEntry>> = if index_path.exists() {
            let content = std::fs::read_to_string(&index_path)?;
            serde_json::from_str(&content).map_err(|e| {
                OxideError::Serialization(format!("Failed to parse model index: {}", e))
            })?
        } else {
            HashMap::new()
        };

        info!("Opened model store at {} ({} models)", root.display(), index.len());

        let store = ModelStore {
            root: root.to_path_buf(),
            index,
        };
        store.save_index()?;
        Ok(store)
    }

    /// Add a model to the store by copying it from a source path.
    pub fn add(
        &mut self,
        source: &Path,
        model_id: ModelId,
        version: ModelVersion,
    ) -> Result<ModelEntry> {
        // Read the file and compute hash
        let data = std::fs::read(source)?;
        let sha256 = Self::sha256_hex(&data);
        let size_bytes = data.len() as u64;
        let format = ModelFormat::from_extension(&source.display().to_string());

        // Determine storage path
        let ext = source
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("bin");
        let rel_path = PathBuf::from(format!("{}/{}.{}", model_id.0, version.0, ext));
        let abs_path = self.root.join(&rel_path);

        // Create directory and copy file
        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&abs_path, &data)?;

        let entry = ModelEntry {
            id: model_id.clone(),
            version: version.clone(),
            format,
            path: rel_path,
            size_bytes,
            sha256,
            added_at: chrono::Utc::now(),
        };

        // Update index
        self.index
            .entry(model_id.clone())
            .or_default()
            .push(entry.clone());

        self.save_index()?;

        info!(
            "Added model '{}' version '{}' ({} bytes)",
            model_id, version, size_bytes
        );

        Ok(entry)
    }

    /// Get the absolute path to a model file.
    pub fn get_path(&self, model_id: &ModelId, version: &ModelVersion) -> Result<PathBuf> {
        let entries = self.index.get(model_id).ok_or_else(|| {
            OxideError::ModelNotFound(model_id.to_string())
        })?;

        let entry = entries
            .iter()
            .find(|e| &e.version == version)
            .ok_or_else(|| {
                OxideError::ModelNotFound(format!("{}@{}", model_id, version))
            })?;

        Ok(self.root.join(&entry.path))
    }

    /// Get the latest version of a model.
    pub fn get_latest(&self, model_id: &ModelId) -> Result<&ModelEntry> {
        let entries = self.index.get(model_id).ok_or_else(|| {
            OxideError::ModelNotFound(model_id.to_string())
        })?;

        entries.last().ok_or_else(|| {
            OxideError::ModelNotFound(format!("{} (no versions)", model_id))
        })
    }

    /// List all models in the store.
    pub fn list(&self) -> Vec<(&ModelId, &Vec<ModelEntry>)> {
        self.index.iter().collect()
    }

    /// List versions of a specific model.
    pub fn list_versions(&self, model_id: &ModelId) -> Result<&Vec<ModelEntry>> {
        self.index.get(model_id).ok_or_else(|| {
            OxideError::ModelNotFound(model_id.to_string())
        })
    }

    /// Get the previous version of a model (for rollback).
    pub fn get_previous_version(
        &self,
        model_id: &ModelId,
        current_version: &ModelVersion,
    ) -> Result<&ModelEntry> {
        let entries = self.index.get(model_id).ok_or_else(|| {
            OxideError::ModelNotFound(model_id.to_string())
        })?;

        let current_idx = entries
            .iter()
            .position(|e| &e.version == current_version)
            .ok_or_else(|| {
                OxideError::ModelNotFound(format!("{}@{}", model_id, current_version))
            })?;

        if current_idx == 0 {
            return Err(OxideError::Rollback(format!(
                "No previous version for {}@{}",
                model_id, current_version
            )));
        }

        Ok(&entries[current_idx - 1])
    }

    /// Verify integrity of a stored model.
    pub fn verify(&self, model_id: &ModelId, version: &ModelVersion) -> Result<bool> {
        let entries = self.index.get(model_id).ok_or_else(|| {
            OxideError::ModelNotFound(model_id.to_string())
        })?;

        let entry = entries
            .iter()
            .find(|e| &e.version == version)
            .ok_or_else(|| {
                OxideError::ModelNotFound(format!("{}@{}", model_id, version))
            })?;

        let abs_path = self.root.join(&entry.path);
        let data = std::fs::read(&abs_path)?;
        let hash = Self::sha256_hex(&data);

        Ok(hash == entry.sha256)
    }

    /// Store root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn save_index(&self) -> Result<()> {
        let path = self.root.join("index.json");
        let content = serde_json::to_string_pretty(&self.index)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    fn sha256_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, ModelStore) {
        let dir = TempDir::new().unwrap();
        let store = ModelStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_store_creation() {
        let (dir, store) = setup();
        assert!(store.list().is_empty());
        assert!(dir.path().join("index.json").exists());
    }

    #[test]
    fn test_add_and_retrieve() {
        let (dir, mut store) = setup();

        // Create a fake model file
        let model_path = dir.path().join("test.onnx");
        std::fs::write(&model_path, b"fake onnx model data").unwrap();

        let entry = store
            .add(
                &model_path,
                ModelId::from("test-model"),
                ModelVersion::from("v1.0.0"),
            )
            .unwrap();

        assert_eq!(entry.id.0, "test-model");
        assert_eq!(entry.version.0, "v1.0.0");
        assert_eq!(entry.format, ModelFormat::Onnx);
        assert_eq!(entry.size_bytes, 20);

        // Retrieve path
        let path = store
            .get_path(&ModelId::from("test-model"), &ModelVersion::from("v1.0.0"))
            .unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_versioning() {
        let (dir, mut store) = setup();

        let model_path = dir.path().join("test.onnx");
        std::fs::write(&model_path, b"v1 data").unwrap();
        store
            .add(
                &model_path,
                ModelId::from("model"),
                ModelVersion::from("v1.0.0"),
            )
            .unwrap();

        std::fs::write(&model_path, b"v2 data").unwrap();
        store
            .add(
                &model_path,
                ModelId::from("model"),
                ModelVersion::from("v2.0.0"),
            )
            .unwrap();

        let latest = store.get_latest(&ModelId::from("model")).unwrap();
        assert_eq!(latest.version.0, "v2.0.0");

        let versions = store.list_versions(&ModelId::from("model")).unwrap();
        assert_eq!(versions.len(), 2);
    }

    #[test]
    fn test_rollback() {
        let (dir, mut store) = setup();

        let model_path = dir.path().join("test.onnx");
        std::fs::write(&model_path, b"v1 data").unwrap();
        store
            .add(
                &model_path,
                ModelId::from("model"),
                ModelVersion::from("v1.0.0"),
            )
            .unwrap();

        std::fs::write(&model_path, b"v2 data").unwrap();
        store
            .add(
                &model_path,
                ModelId::from("model"),
                ModelVersion::from("v2.0.0"),
            )
            .unwrap();

        let prev = store
            .get_previous_version(&ModelId::from("model"), &ModelVersion::from("v2.0.0"))
            .unwrap();
        assert_eq!(prev.version.0, "v1.0.0");

        // No previous for first version
        let err = store
            .get_previous_version(&ModelId::from("model"), &ModelVersion::from("v1.0.0"));
        assert!(err.is_err());
    }

    #[test]
    fn test_integrity_verification() {
        let (dir, mut store) = setup();

        let model_path = dir.path().join("test.onnx");
        std::fs::write(&model_path, b"model data").unwrap();
        store
            .add(
                &model_path,
                ModelId::from("model"),
                ModelVersion::from("v1.0.0"),
            )
            .unwrap();

        assert!(store
            .verify(&ModelId::from("model"), &ModelVersion::from("v1.0.0"))
            .unwrap());
    }

    #[test]
    fn test_persistence() {
        let dir = TempDir::new().unwrap();

        // Create store and add model
        {
            let mut store = ModelStore::open(dir.path()).unwrap();
            let model_path = dir.path().join("test.onnx");
            std::fs::write(&model_path, b"data").unwrap();
            store
                .add(
                    &model_path,
                    ModelId::from("model"),
                    ModelVersion::from("v1.0.0"),
                )
                .unwrap();
        }

        // Reopen store and verify
        {
            let store = ModelStore::open(dir.path()).unwrap();
            assert_eq!(store.list().len(), 1);
            let latest = store.get_latest(&ModelId::from("model")).unwrap();
            assert_eq!(latest.version.0, "v1.0.0");
        }
    }
}
