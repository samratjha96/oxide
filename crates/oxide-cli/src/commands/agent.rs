//! `oxide agent` — Device-side daemon that polls the control plane for model updates.

use oxide_core::device::{HeartbeatRequest, HeartbeatResponse, UpdateResult};
use oxide_core::model::{ModelId, ModelVersion};
use oxide_network::ota::{OtaUpdater, UpdatePackage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::watch;

/// Persisted agent state across restarts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AgentState {
    device_id: String,
    current_model: Option<String>,
    current_model_version: Option<String>,
    model_path: Option<String>,
    last_heartbeat: Option<String>,
    last_update: Option<String>,
}

pub async fn execute(
    control_plane: &str,
    device_id: &str,
    poll_interval: u64,
    model_dir: &str,
    health_check_cmd: Option<&str>,
) -> anyhow::Result<()> {
    let model_dir = PathBuf::from(model_dir);
    std::fs::create_dir_all(&model_dir)?;

    let control_plane = control_plane.trim_end_matches('/');
    let state_file = model_dir.join(".agent-state.json");

    println!("⚡ oxide agent");
    println!("  device:     {}", device_id);
    println!("  control:    {}", control_plane);
    println!("  poll:       every {}s", poll_interval);
    println!("  model dir:  {}", model_dir.display());
    if let Some(cmd) = health_check_cmd {
        println!("  health:     {}", cmd);
    } else {
        println!("  health:     file exists (no custom hook)");
    }
    println!();

    // Load persisted state
    let mut agent_state = load_state(&state_file);
    agent_state.device_id = device_id.to_string();

    // Initialize OTA updater
    let ota_dir = model_dir.join("ota");
    let updater = OtaUpdater::new(&ota_dir)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let mut backoff = poll_interval;
    let max_backoff = 300u64; // 5 min cap
    let mut failed_versions: std::collections::HashMap<(String, String), u32> =
        std::collections::HashMap::new();

    // Shutdown signal
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = shutdown_tx.send(true);
    });

    loop {
        // Sleep with shutdown awareness
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(backoff)) => {},
            _ = shutdown_rx.changed() => {
                println!("\n  shutting down...");
                save_state(&state_file, &agent_state);
                println!("  state saved. goodbye.");
                return Ok(());
            }
        }

        let now = chrono::Utc::now().format("%H:%M:%S").to_string();

        // 1. Heartbeat
        let hb_req = HeartbeatRequest {
            current_model: agent_state
                .current_model
                .as_ref()
                .map(|s| ModelId(s.clone())),
            current_model_version: agent_state
                .current_model_version
                .as_ref()
                .map(|s| ModelVersion(s.clone())),
            status: Some("online".to_string()),
            last_update_result: None,
            metrics: None,
        };

        let hb_url = format!(
            "{}/api/v1/devices/{}/heartbeat",
            control_plane, device_id
        );

        let resp = match client.post(&hb_url).json(&hb_req).send().await {
            Ok(r) => r,
            Err(e) => {
                println!("  [{}] ✗ heartbeat failed: {}", now, e);
                backoff = (backoff * 2).min(max_backoff);
                continue;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            println!(
                "  [{}] ✗ heartbeat returned {}: {}",
                now, status, body
            );
            backoff = (backoff * 2).min(max_backoff);
            continue;
        }

        // Reset backoff on success
        backoff = poll_interval;

        let hb_resp: HeartbeatResponse = match resp.json().await {
            Ok(r) => r,
            Err(e) => {
                println!("  [{}] ✗ bad heartbeat response: {}", now, e);
                continue;
            }
        };

        agent_state.last_heartbeat = Some(chrono::Utc::now().to_rfc3339());

        // 2. Check assignment
        let assigned_model = hb_resp.assigned_model.as_ref().map(|m| m.0.clone());
        let assigned_version = hb_resp
            .assigned_model_version
            .as_ref()
            .map(|v| v.0.clone());

        match (&assigned_model, &assigned_version) {
            (Some(m), Some(v)) => {
                let is_current = agent_state.current_model.as_deref() == Some(m.as_str())
                    && agent_state.current_model_version.as_deref() == Some(v.as_str());

                if is_current {
                    println!("  [{}] heartbeat ok — model current ({}@{})", now, m, v);
                    continue;
                }

                // Check poison pill
                let key = (m.clone(), v.clone());
                let attempts = failed_versions.get(&key).copied().unwrap_or(0);
                if attempts >= 3 {
                    println!(
                        "  [{}] heartbeat ok — skipping {}@{} (failed {} times)",
                        now, m, v, attempts
                    );
                    continue;
                }

                println!(
                    "  [{}] heartbeat ok — assigned {}@{}",
                    now, m, v
                );

                // 3. Download model (delta-aware)
                let dl_url = format!(
                    "{}/api/v1/models/{}/versions/{}/download",
                    control_plane, m, v
                );
                println!("  [{}] downloading {}@{}...", now, m, v);

                // Build download request with delta headers
                let mut dl_request = client.get(&dl_url);
                if let Some(ref current_ver) = agent_state.current_model_version {
                    dl_request = dl_request.header("X-Oxide-Base-Version", current_ver);
                }

                let dl_resp = match dl_request.send().await {
                    Ok(r) if r.status().is_success() => r,
                    Ok(r) => {
                        println!(
                            "  [{}] ✗ download failed: HTTP {}",
                            now,
                            r.status()
                        );
                        continue;
                    }
                    Err(e) => {
                        println!("  [{}] ✗ download failed: {}", now, e);
                        continue;
                    }
                };

                let content_type = dl_resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("application/octet-stream")
                    .to_string();

                let is_delta = content_type == "application/x-oxide-delta";

                let expected_sha = if is_delta {
                    dl_resp
                        .headers()
                        .get("x-oxide-target-sha256")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string())
                } else {
                    dl_resp
                        .headers()
                        .get("x-oxide-sha256")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string())
                };

                let response_bytes = match dl_resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        println!("  [{}] ✗ download body failed: {}", now, e);
                        continue;
                    }
                };

                // Reconstruct model bytes from delta or use as-is
                let model_bytes: Vec<u8> = if is_delta {
                    println!(
                        "  [{}] received delta ({} bytes), reconstructing...",
                        now,
                        response_bytes.len()
                    );

                    match reconstruct_from_delta(&response_bytes, &agent_state, &client, &dl_url, &now).await {
                        Ok(data) => {
                            let ratio = response_bytes.len() as f64 / data.len() as f64;
                            let savings = ratio.mul_add(-100.0, 100.0);
                            println!(
                                "  [{}] reconstructed {} bytes ({:.1}% bandwidth saved)",
                                now,
                                data.len(),
                                savings
                            );
                            data
                        }
                        Err(e) => {
                            println!("  [{}] ✗ delta failed: {}", now, e);
                            continue;
                        }
                    }
                } else {
                    response_bytes.to_vec()
                };

                println!(
                    "  [{}] model ready: {} bytes",
                    now,
                    model_bytes.len()
                );

                // Verify SHA-256
                let actual_sha = sha256_hex(&model_bytes);
                if let Some(ref expected) = expected_sha {
                    if &actual_sha != expected {
                        println!(
                            "  [{}] ✗ sha-256 mismatch: expected {}..., got {}...",
                            now,
                            &expected[..8],
                            &actual_sha[..8]
                        );
                        continue;
                    }
                }

                // 4. OTA pipeline: stage → verify → apply → health-check
                let package = UpdatePackage {
                    model_id: ModelId(m.clone()),
                    new_version: ModelVersion(v.clone()),
                    previous_version: agent_state
                        .current_model_version
                        .as_ref()
                        .map(|s| ModelVersion(s.clone())),
                    sha256: actual_sha.clone(),
                    size_bytes: model_bytes.len() as u64,
                    encrypted: false,
                };

                print!("  [{}] staging...", now);
                let mut update_state = match updater.stage_update(&package, &model_bytes) {
                    Ok(s) => {
                        println!(" done");
                        s
                    }
                    Err(e) => {
                        println!(" ✗ failed: {}", e);
                        *failed_versions.entry(key).or_insert(0) += 1;
                        report_failure(&client, &hb_url, &hb_req, &e.to_string()).await;
                        continue;
                    }
                };

                println!("  [{}] verifying... ok (sha-256 match)", now);

                print!("  [{}] applying...", now);
                let active_path = match updater.apply_update(&mut update_state) {
                    Ok(p) => {
                        println!(" done");
                        p
                    }
                    Err(e) => {
                        println!(" ✗ failed: {}", e);
                        *failed_versions.entry(key).or_insert(0) += 1;
                        report_failure(&client, &hb_url, &hb_req, &e.to_string()).await;
                        continue;
                    }
                };

                // 5. Health check
                print!("  [{}] health check...", now);
                match run_health_check(&active_path, health_check_cmd) {
                    Ok(msg) => {
                        println!(" passed ({})", msg);
                    }
                    Err(e) => {
                        println!(" ✗ failed: {}", e);
                        // Rollback
                        if let Some(prev_ver) = &agent_state.current_model_version {
                            let _ = updater.rollback(
                                &ModelId(m.clone()),
                                &ModelVersion(prev_ver.clone()),
                            );
                            println!("  [{}] rolled back to {}@{}", now, m, prev_ver);
                        }
                        *failed_versions.entry(key).or_insert(0) += 1;
                        report_failure(&client, &hb_url, &hb_req, &e.to_string()).await;
                        continue;
                    }
                }

                // 6. Update state
                agent_state.current_model = Some(m.clone());
                agent_state.current_model_version = Some(v.clone());
                agent_state.model_path = Some(active_path.display().to_string());
                agent_state.last_update = Some(chrono::Utc::now().to_rfc3339());
                save_state(&state_file, &agent_state);

                // Clear any failed attempts for this version (it worked now)
                failed_versions.remove(&(m.clone(), v.clone()));

                println!(
                    "  [{}] ✓ model active: {}@{}",
                    now, m, v
                );
            }
            _ => {
                println!("  [{}] heartbeat ok — no model assigned", now);
            }
        }
    }
}

/// Run a health check on the newly applied model.
///
/// If a custom command is provided, runs it with `OXIDE_MODEL_PATH` set.
/// Exit code 0 = healthy. Otherwise, checks the file exists and is non-empty.
fn run_health_check(model_path: &Path, custom_cmd: Option<&str>) -> anyhow::Result<String> {
    if let Some(cmd) = custom_cmd {
        let output = std::process::Command::new("sh")
            .args(["-c", cmd])
            .env("OXIDE_MODEL_PATH", model_path)
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = stdout.trim();
            if msg.is_empty() {
                Ok("exit 0".to_string())
            } else {
                Ok(msg.to_string())
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "health check exited {}: {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            );
        }
    } else {
        // Default: just check the file exists and is non-empty
        let meta = std::fs::metadata(model_path)?;
        if meta.len() == 0 {
            anyhow::bail!("model file is empty");
        }
        Ok(format!("{} bytes on disk", meta.len()))
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn load_state(path: &Path) -> AgentState {
    if path.exists() {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        AgentState::default()
    }
}

fn save_state(path: &Path, state: &AgentState) {
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(path, json);
    }
}

async fn report_failure(
    client: &reqwest::Client,
    hb_url: &str,
    base_req: &HeartbeatRequest,
    error: &str,
) {
    let mut req = base_req.clone();
    req.last_update_result = Some(UpdateResult::Failed {
        error: error.to_string(),
    });
    let _ = client.post(hb_url).json(&req).send().await;
}

/// Attempt to reconstruct a model from a delta patch.
///
/// Reads the base model from disk, parses the delta, and applies it.
/// On any failure, falls back to a full download.
async fn reconstruct_from_delta(
    delta_bytes: &[u8],
    agent_state: &AgentState,
    client: &reqwest::Client,
    download_url: &str,
    now: &str,
) -> anyhow::Result<Vec<u8>> {
    // Read base model from disk
    let base_path = agent_state
        .model_path
        .as_ref()
        .map(PathBuf::from);

    let base_data = match &base_path {
        Some(p) if p.exists() => std::fs::read(p)?,
        _ => {
            println!(
                "  [{}] no base model on disk, falling back to full download",
                now
            );
            return full_download(client, download_url).await;
        }
    };

    // Parse and apply delta
    let patch = match oxide_delta::DeltaPatch::from_bytes(delta_bytes) {
        Ok(p) => p,
        Err(e) => {
            println!(
                "  [{}] delta parse failed: {}, falling back to full download",
                now, e
            );
            return full_download(client, download_url).await;
        }
    };

    match oxide_delta::apply_delta(&base_data, &patch) {
        Ok(reconstructed) => Ok(reconstructed),
        Err(e) => {
            println!(
                "  [{}] delta apply failed: {}, falling back to full download",
                now, e
            );
            full_download(client, download_url).await
        }
    }
}

/// Full model download (no delta headers).
async fn full_download(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("full download failed: HTTP {}", resp.status());
    }
    Ok(resp.bytes().await?.to_vec())
}
