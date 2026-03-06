//! Campaign tracking for fleet deployments.
//!
//! A campaign is an ongoing deployment with per-device progress tracking.
//! Replaces fire-and-forget deploy with observable, pausable rollouts.

use oxide_core::device::DeviceId;
use oxide_core::fleet::FleetId;
use oxide_core::model::{ModelId, ModelVersion};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique campaign identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CampaignId(pub String);

impl std::fmt::Display for CampaignId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Top-level campaign state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CampaignState {
    /// Created but not yet started.
    Pending,
    /// Actively rolling out to devices.
    RollingOut,
    /// Paused by operator.
    Paused,
    /// All devices reached terminal state.
    Complete,
    /// Aborted by operator.
    Aborted,
}

/// Per-device update state within a campaign.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceUpdateState {
    /// Waiting for device to check in.
    Pending,
    /// Device acknowledged assignment, downloading.
    Downloading,
    /// Device is applying the update.
    Applying,
    /// Device is running health checks.
    Verifying,
    /// Update succeeded.
    Complete {
        completed_at: String,
        bytes_downloaded: u64,
    },
    /// Update failed (may retry).
    Failed { error: String, attempts: u32 },
    /// Skipped (already on target version).
    Skipped,
}

/// A deployment campaign.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub id: CampaignId,
    pub model_id: ModelId,
    pub target_version: ModelVersion,
    pub fleet_id: FleetId,
    pub state: CampaignState,
    pub created_at: String,

    /// Per-device tracking.
    pub devices: HashMap<DeviceId, DeviceUpdateState>,

    /// Bandwidth stats.
    pub total_bytes_served: u64,
    pub total_bytes_saved_by_delta: u64,
}

impl Campaign {
    /// Create a new campaign for a fleet.
    pub fn new(
        id: CampaignId,
        model_id: ModelId,
        target_version: ModelVersion,
        fleet_id: FleetId,
        device_ids: Vec<DeviceId>,
    ) -> Self {
        let mut devices = HashMap::with_capacity(device_ids.len());
        for did in device_ids {
            devices.insert(did, DeviceUpdateState::Pending);
        }
        Self {
            id,
            model_id,
            target_version,
            fleet_id,
            state: CampaignState::RollingOut,
            created_at: chrono::Utc::now().to_rfc3339(),
            devices,
            total_bytes_served: 0,
            total_bytes_saved_by_delta: 0,
        }
    }

    /// Update a device's state within this campaign.
    pub fn update_device(&mut self, device_id: &DeviceId, new_state: DeviceUpdateState) {
        self.devices.insert(device_id.clone(), new_state);
        self.maybe_complete();
    }

    /// Record bytes served for a device download.
    pub const fn record_download(&mut self, bytes_served: u64, bytes_saved: u64) {
        self.total_bytes_served += bytes_served;
        self.total_bytes_saved_by_delta += bytes_saved;
    }

    /// Pause the campaign.
    pub fn pause(&mut self) {
        if self.state == CampaignState::RollingOut {
            self.state = CampaignState::Paused;
        }
    }

    /// Resume a paused campaign.
    pub fn resume(&mut self) {
        if self.state == CampaignState::Paused {
            self.state = CampaignState::RollingOut;
        }
    }

    /// Abort the campaign.
    pub const fn abort(&mut self) {
        self.state = CampaignState::Aborted;
    }

    /// Summary statistics.
    pub fn summary(&self) -> CampaignSummary {
        let mut pending = 0;
        let mut in_progress = 0;
        let mut complete = 0;
        let mut failed = 0;
        let mut skipped = 0;

        for state in self.devices.values() {
            match state {
                DeviceUpdateState::Pending => pending += 1,
                DeviceUpdateState::Downloading
                | DeviceUpdateState::Applying
                | DeviceUpdateState::Verifying => in_progress += 1,
                DeviceUpdateState::Complete { .. } => complete += 1,
                DeviceUpdateState::Failed { .. } => failed += 1,
                DeviceUpdateState::Skipped => skipped += 1,
            }
        }

        CampaignSummary {
            total: self.devices.len(),
            pending,
            in_progress,
            complete,
            failed,
            skipped,
            bytes_served: self.total_bytes_served,
            bytes_saved: self.total_bytes_saved_by_delta,
        }
    }

    /// Check if all devices have reached a terminal state.
    fn maybe_complete(&mut self) {
        if self.state != CampaignState::RollingOut {
            return;
        }
        let all_terminal = self.devices.values().all(|s| {
            matches!(
                s,
                DeviceUpdateState::Complete { .. }
                    | DeviceUpdateState::Failed { .. }
                    | DeviceUpdateState::Skipped
            )
        });
        if all_terminal {
            self.state = CampaignState::Complete;
        }
    }
}

/// Summary stats for a campaign.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignSummary {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub complete: usize,
    pub failed: usize,
    pub skipped: usize,
    pub bytes_served: u64,
    pub bytes_saved: u64,
}

/// In-memory campaign store.
#[derive(Default)]
pub struct CampaignStore {
    campaigns: HashMap<CampaignId, Campaign>,
}

impl CampaignStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&mut self, campaign: Campaign) -> &Campaign {
        let id = campaign.id.clone();
        self.campaigns.insert(id.clone(), campaign);
        self.campaigns.get(&id).unwrap()
    }

    pub fn get(&self, id: &CampaignId) -> Option<&Campaign> {
        self.campaigns.get(id)
    }

    pub fn get_mut(&mut self, id: &CampaignId) -> Option<&mut Campaign> {
        self.campaigns.get_mut(id)
    }

    /// Find the active campaign for a device (if any).
    pub fn active_for_device(&self, device_id: &DeviceId) -> Option<&Campaign> {
        self.campaigns.values().find(|c| {
            c.state == CampaignState::RollingOut
                && c.devices.contains_key(device_id)
                && matches!(
                    c.devices.get(device_id),
                    Some(DeviceUpdateState::Pending)
                        | Some(DeviceUpdateState::Downloading)
                        | Some(DeviceUpdateState::Applying)
                        | Some(DeviceUpdateState::Verifying)
                )
        })
    }

    /// Find the active campaign for a device (mutable).
    pub fn active_for_device_mut(&mut self, device_id: &DeviceId) -> Option<&mut Campaign> {
        self.campaigns.values_mut().find(|c| {
            c.state == CampaignState::RollingOut
                && c.devices.contains_key(device_id)
                && matches!(
                    c.devices.get(device_id),
                    Some(DeviceUpdateState::Pending)
                        | Some(DeviceUpdateState::Downloading)
                        | Some(DeviceUpdateState::Applying)
                        | Some(DeviceUpdateState::Verifying)
                )
        })
    }

    pub fn list(&self) -> Vec<&Campaign> {
        self.campaigns.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campaign_lifecycle() {
        let mut campaign = Campaign::new(
            CampaignId("c1".into()),
            ModelId::from("model"),
            ModelVersion::from("v2"),
            FleetId::from("fleet"),
            vec![DeviceId::from("d1"), DeviceId::from("d2")],
        );

        assert_eq!(campaign.state, CampaignState::RollingOut);
        assert_eq!(campaign.devices.len(), 2);

        let summary = campaign.summary();
        assert_eq!(summary.pending, 2);
        assert_eq!(summary.complete, 0);

        // d1 completes
        campaign.update_device(
            &DeviceId::from("d1"),
            DeviceUpdateState::Complete {
                completed_at: "now".into(),
                bytes_downloaded: 1000,
            },
        );
        assert_eq!(campaign.state, CampaignState::RollingOut); // d2 still pending

        // d2 completes
        campaign.update_device(
            &DeviceId::from("d2"),
            DeviceUpdateState::Complete {
                completed_at: "now".into(),
                bytes_downloaded: 500,
            },
        );
        assert_eq!(campaign.state, CampaignState::Complete); // all done

        let summary = campaign.summary();
        assert_eq!(summary.complete, 2);
        assert_eq!(summary.pending, 0);
    }

    #[test]
    fn campaign_pause_resume() {
        let mut campaign = Campaign::new(
            CampaignId("c2".into()),
            ModelId::from("m"),
            ModelVersion::from("v1"),
            FleetId::from("f"),
            vec![DeviceId::from("d1")],
        );

        campaign.pause();
        assert_eq!(campaign.state, CampaignState::Paused);

        campaign.resume();
        assert_eq!(campaign.state, CampaignState::RollingOut);
    }

    #[test]
    fn campaign_store() {
        let mut store = CampaignStore::new();
        let campaign = Campaign::new(
            CampaignId("c3".into()),
            ModelId::from("m"),
            ModelVersion::from("v1"),
            FleetId::from("f"),
            vec![DeviceId::from("d1")],
        );

        store.create(campaign);
        assert!(store.get(&CampaignId("c3".into())).is_some());
        assert!(store.active_for_device(&DeviceId::from("d1")).is_some());
        assert!(store.active_for_device(&DeviceId::from("d999")).is_none());
    }
}
