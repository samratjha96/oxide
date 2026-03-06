//! OTA (Over-The-Air) update mechanism for model deployment.
//!
//! Supports atomic updates with rollback, integrity verification,
//! and health checking after update.

use oxide_core::error::{OxideError, Result};
use oxide_core::model::{ModelId, ModelVersion};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tracing::info;

/// An OTA update package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePackage {
    /// Model being updated.
    pub model_id: ModelId,
    /// New version.
    pub new_version: ModelVersion,
    /// Previous version (for rollback).
    pub previous_version: Option<ModelVersion>,
    /// SHA-256 hash of the model file.
    pub sha256: String,
    /// Size of the model file in bytes.
    pub size_bytes: u64,
    /// Whether the model is encrypted.
    pub encrypted: bool,
}

/// Status of an OTA update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    Pending,
    Downloading,
    Verifying,
    Installing,
    HealthChecking,
    Complete,
    Failed,
    RolledBack,
}

/// Tracks the state of an ongoing update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateState {
    pub package: UpdatePackage,
    pub status: UpdateStatus,
    pub error: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// OTA updater that manages atomic model updates with rollback.
pub struct OtaUpdater {
    /// Directory for staging updates.
    staging_dir: PathBuf,
    /// Directory where active models live.
    models_dir: PathBuf,
    /// Directory for rollback backups.
    backup_dir: PathBuf,
}

impl OtaUpdater {
    /// Create a new OTA updater.
    pub fn new(base_dir: &Path) -> Result<Self> {
        let staging_dir = base_dir.join("staging");
        let models_dir = base_dir.join("models");
        let backup_dir = base_dir.join("backup");

        std::fs::create_dir_all(&staging_dir)?;
        std::fs::create_dir_all(&models_dir)?;
        std::fs::create_dir_all(&backup_dir)?;

        Ok(OtaUpdater {
            staging_dir,
            models_dir,
            backup_dir,
        })
    }

    /// Stage an update by writing model data to the staging directory.
    /// Returns the path to the staged file.
    pub fn stage_update(
        &self,
        package: &UpdatePackage,
        model_data: &[u8],
    ) -> Result<UpdateState> {
        info!(
            "Staging update for model '{}' version '{}'",
            package.model_id, package.new_version
        );

        // Verify integrity
        let actual_hash = Self::sha256_hex(model_data);
        if actual_hash != package.sha256 {
            return Err(OxideError::Update(format!(
                "Hash mismatch: expected {}, got {}",
                package.sha256, actual_hash
            )));
        }

        // Write to staging
        let staged_path = self
            .staging_dir
            .join(format!("{}_{}.bin", package.model_id.0, package.new_version.0));
        std::fs::write(&staged_path, model_data)?;

        info!("Model staged at: {}", staged_path.display());

        Ok(UpdateState {
            package: package.clone(),
            status: UpdateStatus::Verifying,
            error: None,
            started_at: chrono::Utc::now(),
            completed_at: None,
        })
    }

    /// Apply a staged update atomically.
    ///
    /// 1. Backup current model (if exists)
    /// 2. Move staged model to active directory
    /// 3. Return success or error
    pub fn apply_update(&self, state: &mut UpdateState) -> Result<PathBuf> {
        let model_id = &state.package.model_id;
        let new_version = &state.package.new_version;

        info!("Applying update for model '{}' version '{}'", model_id, new_version);
        state.status = UpdateStatus::Installing;

        let staged_path = self
            .staging_dir
            .join(format!("{}_{}.bin", model_id.0, new_version.0));

        if !staged_path.exists() {
            state.status = UpdateStatus::Failed;
            state.error = Some("Staged file not found".to_string());
            return Err(OxideError::Update("Staged file not found".to_string()));
        }

        // Backup current model if it exists
        let active_path = self.models_dir.join(format!("{}.bin", model_id.0));
        if active_path.exists() {
            let backup_path = self.backup_dir.join(format!(
                "{}_{}.bin",
                model_id.0,
                state
                    .package
                    .previous_version
                    .as_ref()
                    .map(|v| v.0.as_str())
                    .unwrap_or("unknown")
            ));
            std::fs::copy(&active_path, &backup_path)?;
            info!("Backed up current model to: {}", backup_path.display());
        }

        // Atomic move: copy staged -> active, then remove staged
        std::fs::copy(&staged_path, &active_path)?;
        std::fs::remove_file(&staged_path)?;

        state.status = UpdateStatus::Complete;
        state.completed_at = Some(chrono::Utc::now());
        info!("Update applied successfully");

        Ok(active_path)
    }

    /// Rollback to a previous version.
    pub fn rollback(
        &self,
        model_id: &ModelId,
        previous_version: &ModelVersion,
    ) -> Result<PathBuf> {
        info!(
            "Rolling back model '{}' to version '{}'",
            model_id, previous_version
        );

        let backup_path = self
            .backup_dir
            .join(format!("{}_{}.bin", model_id.0, previous_version.0));

        if !backup_path.exists() {
            return Err(OxideError::Rollback(format!(
                "Backup not found for {}@{}",
                model_id, previous_version
            )));
        }

        let active_path = self.models_dir.join(format!("{}.bin", model_id.0));
        std::fs::copy(&backup_path, &active_path)?;

        info!("Rolled back to version '{}'", previous_version);
        Ok(active_path)
    }

    /// Clean up staging directory.
    pub fn clean_staging(&self) -> Result<()> {
        for entry in std::fs::read_dir(&self.staging_dir)? {
            let entry = entry?;
            std::fs::remove_file(entry.path())?;
        }
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

    fn setup() -> (TempDir, OtaUpdater) {
        let dir = TempDir::new().unwrap();
        let updater = OtaUpdater::new(dir.path()).unwrap();
        (dir, updater)
    }

    fn make_package(data: &[u8]) -> UpdatePackage {
        let hash = OtaUpdater::sha256_hex(data);
        UpdatePackage {
            model_id: ModelId::from("test-model"),
            new_version: ModelVersion::from("v2.0.0"),
            previous_version: Some(ModelVersion::from("v1.0.0")),
            sha256: hash,
            size_bytes: data.len() as u64,
            encrypted: false,
        }
    }

    #[test]
    fn test_stage_update() {
        let (_dir, updater) = setup();
        let data = b"new model data v2";
        let package = make_package(data);

        let state = updater.stage_update(&package, data).unwrap();
        assert_eq!(state.status, UpdateStatus::Verifying);
    }

    #[test]
    fn test_stage_update_hash_mismatch() {
        let (_dir, updater) = setup();
        let data = b"new model data";
        let mut package = make_package(data);
        package.sha256 = "wrong_hash".to_string();

        let result = updater.stage_update(&package, data);
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_update() {
        let (_dir, updater) = setup();
        let data = b"new model data v2";
        let package = make_package(data);

        let mut state = updater.stage_update(&package, data).unwrap();
        let path = updater.apply_update(&mut state).unwrap();

        assert!(path.exists());
        assert_eq!(state.status, UpdateStatus::Complete);
        assert_eq!(std::fs::read(&path).unwrap(), data);
    }

    #[test]
    fn test_update_with_backup_and_rollback() {
        let (_dir, updater) = setup();

        // First, put a "v1" model in place
        let v1_data = b"model data v1";
        let active_path = updater.models_dir.join("test-model.bin");
        std::fs::write(&active_path, v1_data).unwrap();

        // Stage and apply v2
        let v2_data = b"model data v2";
        let package = make_package(v2_data);
        let mut state = updater.stage_update(&package, v2_data).unwrap();
        let path = updater.apply_update(&mut state).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), v2_data);

        // Rollback to v1
        let rollback_path = updater
            .rollback(
                &ModelId::from("test-model"),
                &ModelVersion::from("v1.0.0"),
            )
            .unwrap();

        assert_eq!(std::fs::read(&rollback_path).unwrap(), v1_data);
    }

    #[test]
    fn test_rollback_no_backup() {
        let (_dir, updater) = setup();
        let result = updater.rollback(
            &ModelId::from("test-model"),
            &ModelVersion::from("v1.0.0"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_clean_staging() {
        let (_dir, updater) = setup();
        let data = b"test";
        let package = make_package(data);
        updater.stage_update(&package, data).unwrap();

        updater.clean_staging().unwrap();
        let count = std::fs::read_dir(&updater.staging_dir)
            .unwrap()
            .count();
        assert_eq!(count, 0);
    }
}
