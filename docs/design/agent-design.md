# Oxide Agent: Device-Side Daemon — Design Document

**Status:** Implemented (see `crates/oxide-cli/src/commands/agent.rs`)  
**Authors:** Oxide Contributors  
**Last Updated:** 2026-03-06  
**Target:** v0.2.0

> **Note:** This was the design document written before implementation.
> Key changes since: health checks now use `--health-check` command hooks
> (not `InferenceEngine`), the agent supports delta downloads via OXDL,
> and campaigns replace fire-and-forget deploy. See `docs/design/oxide-design.md`
> for the current task checklist.

---

## Table of Contents

1. [Problem Statement](#1-problem-statement)
2. [Goals & Non-Goals](#2-goals--non-goals)
3. [Architecture Overview](#3-architecture-overview)
4. [Agent Lifecycle](#4-agent-lifecycle)
5. [Control Plane Extensions](#5-control-plane-extensions)
6. [Wire Protocol](#6-wire-protocol)
7. [OTA Integration](#7-ota-integration)
8. [CLI Changes](#8-cli-changes)
9. [Code Changes by Crate](#9-code-changes-by-crate)
10. [Data Model Changes](#10-data-model-changes)
11. [Configuration](#11-configuration)
12. [Error Handling & Resilience](#12-error-handling--resilience)
13. [Security Considerations](#13-security-considerations)
14. [Observability](#14-observability)
15. [Testing Strategy](#15-testing-strategy)
16. [Deployment & Operations](#16-deployment--operations)
17. [Known Limitations (Beta)](#17-known-limitations-beta)
18. [Implementation Plan](#18-implementation-plan)
19. [Future Work](#19-future-work)

---

## 1. Problem Statement

Today, `oxide deploy --fleet factory` updates an in-memory/JSON record saying "factory should run model v3." But nothing pushes the model to actual devices. There is no process on each device that:

- Discovers it has a pending model update
- Downloads the model from the control plane
- Applies it through the existing OTA pipeline (stage → verify → backup → apply → health-check)
- Reports the result back

The fleet manager simulates deployment success by checking device status in the registry, but no bytes move. The control plane has no model storage. Devices have no way to call home.

**The agent closes this gap.** It is a long-running daemon on each device that polls the control plane, discovers assignments, downloads models, applies them via the existing OTA engine, and reports back. Combined with a model store on the control plane and an extended heartbeat protocol, this completes Oxide's deployment story end-to-end.

---

## 2. Goals & Non-Goals

### Goals (Internal Beta)

| # | Goal | Rationale |
|---|------|-----------|
| G1 | Device-side daemon that polls control plane and applies model updates | Core feature gap |
| G2 | Model storage on the control plane (filesystem-backed) | Devices need somewhere to download from |
| G3 | Pull-based model delivery via extended heartbeat | Simple, NAT-friendly, offline-tolerant |
| G4 | Full OTA pipeline integration (stage → verify → apply → health-check → rollback) | Already built, just needs wiring |
| G5 | `oxide deploy` uploads models to control plane and sets fleet assignments | Complete the CLI workflow |
| G6 | `oxide agent` subcommand that blocks forever as a daemon | The device entry point |
| G7 | Docker-based demo of full end-to-end flow | Prove it works without hardware |

### Non-Goals (Explicitly Deferred)

| # | Non-Goal | Why |
|---|----------|-----|
| NG1 | Push-based delivery (WebSocket, MQTT, gRPC streaming) | Pull is simpler, works through NATs/firewalls, handles intermittent connectivity naturally |
| NG2 | Delta/diff model updates | Models are 1 KB–2 MB typically; full download is fine up to ~100 MB |
| NG3 | mTLS between agent and control plane | Rustls is a dependency but config wiring is a follow-up; beta runs on trusted networks |
| NG4 | Multi-tenancy, auth, RBAC, API keys | One control plane per deployment team; not a SaaS platform |
| NG5 | Model format conversion on the control plane | Upload ONNX, download ONNX. Pre-process before upload |
| NG6 | Agent-hosted inference endpoint | Agent manages the model file; application loads it independently or links oxide-runtime |
| NG7 | Prometheus metrics export | Telemetry structs exist; the scrape endpoint is a follow-up |

---

## 3. Architecture Overview

```
┌───────────────────────────────────────────────────────┐
│                   CONTROL PLANE                       │
│                oxide serve --port 8080                 │
│                                                       │
│  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐ │
│  │   Device      │  │   Fleet      │  │   Model     │ │
│  │   Registry    │  │   Manager    │  │   Store     │ │
│  │  (JSON disk)  │  │  (JSON disk) │  │  (fs disk)  │ │
│  └──────┬───────┘  └──────┬───────┘  └──────┬──────┘ │
│         │                 │                  │        │
│  ┌──────┴─────────────────┴──────────────────┴──────┐ │
│  │              HTTP API (axum)                      │ │
│  │  /api/v1/devices/:id/heartbeat  ← assignment     │ │
│  │  /api/v1/models/:id/:ver/download ← model bytes  │ │
│  │  /api/v1/fleets/:id/deploy      ← upload+assign  │ │
│  └──────────────────────────────────────────────────┘ │
└───────────────────────┬───────────────────────────────┘
                        │
                   HTTP (pull)
                        │
     ┌──────────────────┴──────────────────┐
     │           OXIDE AGENT               │
     │     oxide agent --device-id cam-01  │
     │                                     │
     │  ┌────────────┐  ┌───────────────┐  │
     │  │  Poll Loop  │  │  OTA Updater  │  │
     │  │  (timer)    │──│  (existing)   │  │
     │  └─────┬──────┘  └───────┬───────┘  │
     │        │                 │           │
     │  ┌─────┴──────┐  ┌──────┴────────┐  │
     │  │  HTTP      │  │  Inference    │  │
     │  │  Client    │  │  Engine       │  │
     │  │  (ureq)    │  │  (health chk) │  │
     │  └────────────┘  └───────────────┘  │
     │                                     │
     │  Models stored at:                  │
     │    <model_dir>/<model_id>.onnx      │
     └─────────────────────────────────────┘
```

### Key design decisions

1. **Pull, not push.** The agent polls on a fixed interval. This works through NATs, firewalls, and intermittent connectivity. The device is always the one initiating the connection.

2. **Filesystem model store, not a database.** Models are files on disk under `<data_dir>/models/<id>/<version>.onnx`. The index is a JSON file. Same pattern as `ModelStore` in oxide-runtime.

3. **Heartbeat carries everything.** The heartbeat request includes current model state and metrics. The heartbeat response includes the model assignment. No separate "check for updates" endpoint. One round-trip does it all.

4. **Reuse existing OTA engine.** The `OtaUpdater` in oxide-network already handles stage → verify → backup → apply → rollback. The agent just feeds it downloaded bytes.

5. **Synchronous agent loop.** The agent's work is fundamentally sequential: sleep → heartbeat → maybe download → maybe apply → sleep. We use tokio since it's already linked (the CLI binary uses `#[tokio::main]`), but the loop itself is a simple `tokio::time::interval` tick.

---

## 4. Agent Lifecycle

### Startup

```
1. Parse config (CLI flags + oxide.toml [agent] section)
2. Validate: device-id is set, control-plane URL is reachable
3. Create local directories: <model_dir>, <model_dir>/staging, <model_dir>/backup
4. Initialize OtaUpdater pointed at <model_dir>
5. If a model already exists at <model_dir>/active/<model_id>.onnx:
   a. Load it into InferenceEngine
   b. Run health check
   c. Record as current_model + current_version
6. Log startup banner
7. Enter poll loop
```

### Poll Loop (steady state)

```
loop {
    sleep(poll_interval)

    // 1. Heartbeat
    let response = http_client.heartbeat(HeartbeatRequest {
        device_id,
        current_model,
        current_model_version,
        status: "online",
        metrics: collect_basic_metrics(),
    })

    if response.is_err() {
        log_warn("heartbeat failed, will retry")
        increment backoff (capped at 5 min)
        continue
    }

    reset backoff to poll_interval

    // 2. Check assignment
    let assigned = response.assigned_model + response.assigned_model_version
    if assigned == current {
        continue  // nothing to do
    }

    if assigned is None {
        continue  // no model assigned yet
    }

    // 3. Download
    let (model_bytes, expected_sha256) = http_client.download_model(
        assigned.model_id,
        assigned.model_version,
    )?

    // 4. Apply via OTA pipeline
    let result = ota_updater.stage_update(package, &model_bytes)?
    let active_path = ota_updater.apply_update(&mut state)?

    // 5. Health check: load model, run test inference
    let engine = InferenceEngine::new(0)
    let info = engine.load_model(&active_path)?
    let test_input = zeros(info.inputs[0].shape)
    engine.infer(&info.id, &test_input, &shape)?

    // 6. Update local state
    current_model = assigned.model_id
    current_model_version = assigned.model_version
    write_state_file()  // persist across restarts

    // If health check failed at step 5:
    //   ota_updater.rollback(previous_version)
    //   current stays at previous
    //   report failure in next heartbeat
}
```

### Shutdown

- SIGTERM/SIGINT: clean exit, log final status
- No special cleanup needed — model files persist on disk, state file persists
- Next startup resumes from persisted state

---

## 5. Control Plane Extensions

### 5.1 Model Store (new module: `oxide-control/src/model_store.rs`)

Filesystem-backed storage for model files that devices download.

```
<data_dir>/
  models/
    defect-detector/
      v1.0.0.onnx
      v2.0.0.onnx
      v2.1.0.onnx
    face-recognition/
      v1.0.0.onnx
  model_index.json        ← { model_id -> [{ version, sha256, size, uploaded_at }] }
```

**Interface:**

```rust
pub struct ControlPlaneModelStore {
    root: PathBuf,
    index: HashMap<ModelId, Vec<StoredModelEntry>>,
}

pub struct StoredModelEntry {
    pub model_id: ModelId,
    pub version: ModelVersion,
    pub sha256: String,
    pub size_bytes: u64,
    pub uploaded_at: DateTime<Utc>,
}

impl ControlPlaneModelStore {
    pub fn open(root: &Path) -> Result<Self>;
    pub fn store(&mut self, id: &ModelId, version: &ModelVersion, data: &[u8]) -> Result<StoredModelEntry>;
    pub fn get_bytes(&self, id: &ModelId, version: &ModelVersion) -> Result<Vec<u8>>;
    pub fn get_meta(&self, id: &ModelId, version: &ModelVersion) -> Result<&StoredModelEntry>;
    pub fn list(&self, id: &ModelId) -> Result<&Vec<StoredModelEntry>>;
}
```

This is deliberately similar to `oxide-runtime::ModelStore` but simpler — no model loading/parsing, just byte storage with hashing.

### 5.2 Extended Heartbeat

The existing heartbeat endpoint (`POST /api/v1/devices/:id/heartbeat`) currently accepts no body and returns `{"status": "ok"}`. It is extended to carry device state inbound and assignments outbound.

**Request body (new):**

```json
{
    "current_model": "defect-detector",
    "current_model_version": "v2.0.0",
    "status": "online",
    "last_update_result": "success",
    "metrics": {
        "inference_count": 14302,
        "avg_latency_us": 31.2,
        "uptime_secs": 86400
    }
}
```

**Response body (extended):**

```json
{
    "status": "ok",
    "assigned_model": "defect-detector",
    "assigned_model_version": "v2.1.0"
}
```

The heartbeat handler:
1. Deserializes the request body (if present — backward compatible with empty body)
2. Updates the device record: `status`, `current_model`, `current_model_version`, `last_heartbeat`
3. Reads the device's `assigned_model` + `assigned_model_version`
4. Returns them in the response

### 5.3 New API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/api/v1/models/{id}/versions/{version}` | Upload model bytes (raw body or multipart) |
| `GET` | `/api/v1/models/{id}/versions/{version}/download` | Download model bytes |
| `GET` | `/api/v1/models/{id}/versions/{version}/meta` | Model metadata (sha256, size) |
| `GET` | `/api/v1/models/{id}` | List all versions of a model |

### 5.4 Fleet Deploy Changes

When `POST /api/v1/fleets/:id/deploy` is called (or `oxide deploy --fleet --control-plane`):

1. Model bytes are uploaded to the `ControlPlaneModelStore` (if not already stored)
2. For each device in the fleet (respecting rollout strategy):
   - Set `device.assigned_model = model_id`
   - Set `device.assigned_model_version = version`
3. Return the deployment result as before

The actual model transfer happens asynchronously when each device's agent polls.

---

## 6. Wire Protocol

All communication is HTTP/1.1 JSON over TCP. The agent is the only initiator.

### 6.1 Heartbeat

```
POST /api/v1/devices/{device_id}/heartbeat
Content-Type: application/json

{
    "current_model": "defect-detector",       // nullable
    "current_model_version": "v2.0.0",        // nullable
    "status": "online",
    "last_update_result": "success",           // "success" | "failed" | null
    "last_update_error": null,                 // error string if failed
    "metrics": {                               // nullable, best-effort
        "inference_count": 14302,
        "avg_latency_us": 31.2,
        "uptime_secs": 86400,
        "free_memory_bytes": 512000000
    }
}

200 OK
{
    "status": "ok",
    "assigned_model": "defect-detector",       // nullable
    "assigned_model_version": "v2.1.0"         // nullable
}
```

**Backward compatibility:** If the body is empty or unparseable, the handler falls back to the current behavior (just update `last_heartbeat` and `status = Online`, return `{"status": "ok"}`). Old agents/curl commands keep working.

### 6.2 Model Download

```
GET /api/v1/models/{model_id}/versions/{version}/download

200 OK
Content-Type: application/octet-stream
X-Oxide-SHA256: a1b2c3d4e5f6...
Content-Length: 2143232

<raw model bytes>
```

The agent verifies the SHA-256 after download before passing to the OTA pipeline.

### 6.3 Model Upload

```
POST /api/v1/models/{model_id}/versions/{version}
Content-Type: application/octet-stream
Content-Length: 2143232

<raw model bytes>

201 Created
{
    "model_id": "defect-detector",
    "version": "v2.1.0",
    "sha256": "a1b2c3d4e5f6...",
    "size_bytes": 2143232
}
```

### 6.4 Model Metadata

```
GET /api/v1/models/{model_id}/versions/{version}/meta

200 OK
{
    "model_id": "defect-detector",
    "version": "v2.1.0",
    "sha256": "a1b2c3d4e5f6...",
    "size_bytes": 2143232,
    "uploaded_at": "2026-03-06T12:00:00Z"
}
```

---

## 7. OTA Integration

The agent reuses the existing `OtaUpdater` from oxide-network verbatim. The flow:

```
Agent downloads model_bytes from control plane
                    │
                    ▼
   ┌─────────────────────────────────────┐
   │  OtaUpdater::stage_update(pkg, bytes)│ ← writes to staging/, verifies SHA-256
   └──────────────────┬──────────────────┘
                      │
                      ▼
   ┌─────────────────────────────────────┐
   │  OtaUpdater::apply_update(&mut st)  │ ← backup current → apply staged → cleanup
   └──────────────────┬──────────────────┘
                      │
                      ▼
   ┌─────────────────────────────────────┐
   │  Health Check                        │ ← load model via InferenceEngine
   │  - OnnxModel::load(active_path)      │    run inference with zero input
   │  - engine.infer(model_id, zeros, sh) │    verify output has expected dims
   └──────────────────┬──────────────────┘
                      │
              ┌───────┴───────┐
              │               │
           success         failure
              │               │
              ▼               ▼
   current_model =    OtaUpdater::rollback()
   assigned_model     current_model stays
                      report error in next heartbeat
```

The `UpdatePackage` is constructed from the download metadata:

```rust
let package = UpdatePackage {
    model_id: assigned_model.clone(),
    new_version: assigned_version.clone(),
    previous_version: current_model_version.clone(), // for rollback
    sha256: response_sha256,
    size_bytes: model_bytes.len() as u64,
    encrypted: false,
};
```

### Encrypted model support

If `--key <path>` is provided:
1. Download encrypted bytes from control plane
2. Decrypt locally using `oxide_security::decrypt_data`
3. Pass decrypted bytes to OTA pipeline

The control plane stores whatever bytes were uploaded. Encryption is the uploader's responsibility (`oxide encrypt` before deploy, or `oxide deploy` gains a `--encrypt-key` flag).

---

## 8. CLI Changes

### New subcommand: `oxide agent`

```
oxide agent [OPTIONS]

Options:
    --control-plane <URL>      Control plane URL (required)
                               Example: http://10.0.1.50:8080

    --device-id <ID>           This device's ID (required, must be pre-registered)

    --poll-interval <SECS>     How often to check in [default: 30]

    --model-dir <PATH>         Local model storage directory [default: ./models]

    --key <PATH>               Encryption key for encrypted model downloads (optional)

    --state-file <PATH>        Path to persist agent state across restarts
                               [default: <model_dir>/.agent-state.json]

    --max-download-bytes <N>   Maximum model download size [default: 536870912 (512MB)]

    --health-check-timeout <S> Seconds to wait for health check [default: 30]
```

The agent blocks forever. Designed to run as:
- `oxide agent --control-plane ... --device-id ...` (foreground)
- `systemd` service with `ExecStart=/usr/local/bin/oxide agent ...`
- Docker `ENTRYPOINT ["oxide", "agent", ...]`

### Modified subcommand: `oxide deploy`

New optional flag:

```
    --control-plane <URL>      Upload model to control plane and set fleet assignments
                               (omit for local-only deploy as today)

    --version <VERSION>        Model version string [default: auto-generated from sha256 prefix]
```

Behavior when `--control-plane` is provided:
1. Read model file from disk
2. Compute SHA-256
3. Upload to `POST /api/v1/models/{id}/versions/{version}`
4. Call `POST /api/v1/fleets/{fleet}/deploy` with the model ID and version
5. Print summary (how many devices will receive the update on next poll)

### main.rs changes

```rust
// Add to Commands enum:
Agent {
    #[arg(long)]
    control_plane: String,

    #[arg(long)]
    device_id: String,

    #[arg(long, default_value = "30")]
    poll_interval: u64,

    #[arg(long, default_value = "./models")]
    model_dir: String,

    #[arg(long)]
    key: Option<String>,
},
```

---

## 9. Code Changes by Crate

### oxide-core

| File | Change |
|------|--------|
| `device.rs` | Add `assigned_model: Option<ModelId>` and `assigned_model_version: Option<ModelVersion>` to `Device` |
| `device.rs` | Add `HeartbeatRequest` struct (serde) |
| `device.rs` | Add `HeartbeatResponse` struct (serde) |

**Impact:** ~40 lines. All new fields are `Option` so existing serialized data deserializes cleanly (serde defaults to `None`).

### oxide-control

| File | Change |
|------|--------|
| `model_store.rs` **(new)** | `ControlPlaneModelStore` — filesystem model storage, ~150 lines |
| `server.rs` | Add model upload/download/meta endpoints, ~80 lines |
| `server.rs` | Extend heartbeat handler to accept body and return assignments, ~30 lines |
| `server.rs` | Add `ControlPlaneModelStore` to `ControlPlaneState`, ~5 lines |
| `fleet_manager.rs` | `deploy()` sets `assigned_model` + `assigned_model_version` on each device in the registry, ~20 lines |
| `lib.rs` | Export `model_store` module |

**Impact:** ~285 lines new, ~50 lines modified.

### oxide-network

| File | Change |
|------|--------|
| `client.rs` **(new)** | `AgentClient` — HTTP client for heartbeat, model download. Uses `ureq` (sync), ~120 lines |
| `lib.rs` | Export `client` module |

**Impact:** ~130 lines new.

### oxide-cli

| File | Change |
|------|--------|
| `commands/agent.rs` **(new)** | Agent loop implementation, ~200 lines |
| `commands/deploy.rs` | Add `--control-plane` path that uploads + assigns, ~60 lines |
| `main.rs` | Add `Agent` variant to `Commands` enum, ~15 lines |
| `commands/mod.rs` | Add `pub mod agent;` |

**Impact:** ~275 lines new, ~15 lines modified.

### New dependency

```toml
# workspace Cargo.toml
ureq = "3"   # synchronous HTTP client, ~200KB, pure Rust
```

**Why ureq over reqwest:** The agent's HTTP needs are three simple request/response calls. `ureq` is synchronous, has no async runtime dependency, compiles fast, and produces a smaller binary. The control plane uses axum/tokio for the server side; the agent client doesn't need that.

### Total estimated changes

| Category | Lines |
|----------|------:|
| New code | ~690 |
| Modified code | ~65 |
| New tests | ~350 |
| **Total** | **~1,105** |

---

## 10. Data Model Changes

### Device (oxide-core/src/device.rs)

```rust
pub struct Device {
    // ... existing fields unchanged ...

    /// Model the control plane wants this device to run (set by fleet deploy).
    pub assigned_model: Option<ModelId>,

    /// Version the control plane wants this device to run.
    pub assigned_model_version: Option<ModelVersion>,

    /// Result of the last update attempt.
    pub last_update_result: Option<UpdateResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpdateResult {
    Success,
    Failed { error: String },
}
```

### Agent State File (`<model_dir>/.agent-state.json`)

Persisted to disk so the agent resumes correctly after restart.

```json
{
    "device_id": "cam-01",
    "current_model": "defect-detector",
    "current_model_version": "v2.1.0",
    "model_path": "./models/active/defect-detector.onnx",
    "last_heartbeat": "2026-03-06T12:30:00Z",
    "last_update": "2026-03-06T12:15:00Z"
}
```

### Heartbeat Request/Response (oxide-core)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub current_model: Option<ModelId>,
    pub current_model_version: Option<ModelVersion>,
    pub status: DeviceStatus,
    pub last_update_result: Option<UpdateResult>,
    pub metrics: Option<BasicMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub status: String,
    pub assigned_model: Option<ModelId>,
    pub assigned_model_version: Option<ModelVersion>,
}

/// Lightweight metrics sent with every heartbeat (not the full InferenceMetrics).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicMetrics {
    pub inference_count: u64,
    pub avg_latency_us: f64,
    pub uptime_secs: u64,
    pub free_memory_bytes: Option<u64>,
}
```

---

## 11. Configuration

### oxide.toml — new `[agent]` section

```toml
[agent]
control_plane_url = "http://10.0.1.50:8080"
device_id = "cam-01"
poll_interval_secs = 30
model_dir = "./models"
# key_file = "./oxide.key"           # optional, for encrypted models
max_download_bytes = 536870912       # 512 MB
health_check_timeout_secs = 30
```

### Precedence

CLI flags > environment variables > `oxide.toml` > defaults.

| Setting | CLI Flag | Env Var | Config Key | Default |
|---------|----------|---------|------------|---------|
| Control plane URL | `--control-plane` | `OXIDE_CONTROL_PLANE` | `agent.control_plane_url` | (required) |
| Device ID | `--device-id` | `OXIDE_DEVICE_ID` | `agent.device_id` | (required) |
| Poll interval | `--poll-interval` | `OXIDE_POLL_INTERVAL` | `agent.poll_interval_secs` | `30` |
| Model directory | `--model-dir` | `OXIDE_MODEL_DIR` | `agent.model_dir` | `./models` |
| Encryption key | `--key` | `OXIDE_KEY_FILE` | `agent.key_file` | None |

---

## 12. Error Handling & Resilience

### Network failures

The agent must tolerate a completely unreliable network. Devices may be on cellular, behind NATs, or powered off for hours.

| Failure | Behavior |
|---------|----------|
| Heartbeat timeout/error | Log warning, increment backoff, retry next cycle |
| Model download interrupted | Discard partial download, retry on next poll |
| Model download hash mismatch | Reject, log error, retry on next poll (server may be mid-upload) |
| Control plane unreachable for extended period | Backoff caps at 5 minutes, current model keeps running |

**Backoff strategy:** Exponential backoff starting at `poll_interval`, capped at 5 minutes. Resets to `poll_interval` on any successful heartbeat.

```
attempt 1: 30s (normal poll)
attempt 2: 60s (after failure)
attempt 3: 120s
attempt 4: 240s
attempt 5+: 300s (cap)
successful heartbeat: reset to 30s
```

### OTA failures

| Failure | Behavior |
|---------|----------|
| Stage fails (disk full, permission error) | Log error, report in next heartbeat, do not retry until next assignment change |
| SHA-256 verification fails | Reject staged model, log error, retry download on next poll |
| Apply fails (IO error during copy) | Rollback to previous model (if exists), report failure |
| Health check fails (model won't load, inference crashes) | Rollback via `OtaUpdater::rollback()`, report failure |

### State recovery

On startup, the agent reads `.agent-state.json` to recover:
- If no state file exists → fresh start, no current model
- If state file references a model that no longer exists on disk → clear current model, will re-download on next heartbeat
- If state file is corrupt → treat as fresh start, log warning

### Poison pill protection

If a model repeatedly fails health checks:
- The agent does NOT retry the same model+version more than 3 times
- After 3 failures, it marks the version as "rejected" locally and stops attempting it
- A new version assignment from the control plane resets the counter
- This prevents infinite download-fail-rollback-download loops

```rust
struct RejectedVersions {
    entries: HashMap<(ModelId, ModelVersion), u32>,  // attempt count
    max_attempts: u32,                                // default: 3
}
```

---

## 13. Security Considerations

### Beta scope (what we ship)

| Area | Approach |
|------|----------|
| Transport | Plain HTTP. Acceptable for beta on trusted internal networks. |
| Model integrity | SHA-256 verification on every download, checked both by agent and OTA pipeline. |
| Model encryption at rest | Optional via `--key`. Uses existing AES-256-GCM from oxide-security. |
| Authentication | None. Device ID is self-asserted. Acceptable for beta. |

### Threat model (beta)

| Threat | Mitigated? | Notes |
|--------|:----------:|-------|
| Model tampering in transit | ✅ | SHA-256 verified after download |
| Model theft from device disk | ⚠️ Optional | `--key` enables AES-256-GCM encryption at rest |
| Rogue device impersonating another | ❌ | No auth in beta; mitigated by network isolation |
| MITM between agent and control plane | ❌ | No TLS in beta; mitigated by running on trusted network |
| Control plane compromise | ❌ | No integrity signing of model provenance |

### Post-beta plan

1. **TLS:** Add `--tls-cert` / `--tls-key` to `oxide serve`, and `--ca-cert` to `oxide agent`. Rustls is already a dependency.
2. **mTLS:** Each device gets a client certificate. The control plane verifies device identity.
3. **API tokens:** Simple bearer token for device authentication, generated at registration time.

---

## 14. Observability

### Agent logging

The agent uses `tracing` (already integrated) with structured fields:

```
INFO  oxide_agent: starting device_id="cam-01" control_plane="http://10.0.1.50:8080" poll_interval=30s
INFO  oxide_agent: heartbeat ok device_id="cam-01" assigned_model="defect-detector" assigned_version="v2.1.0"
INFO  oxide_agent: downloading model model_id="defect-detector" version="v2.1.0" size=2143232
INFO  oxide_agent: staging model... done elapsed=8ms
INFO  oxide_agent: verifying integrity... ok sha256="a1b2c3..."
INFO  oxide_agent: applying update... done elapsed=3ms
INFO  oxide_agent: health check passed latency_us=29 outputs=10
INFO  oxide_agent: model active model_id="defect-detector" version="v2.1.0"
WARN  oxide_agent: heartbeat failed error="connection refused" next_retry=60s
ERROR oxide_agent: health check failed error="inference produced 0 outputs" rolling_back=true
```

### Console output

When run interactively, the agent prints a human-friendly banner and status:

```
⚡ oxide agent
  device:     cam-01
  control:    http://10.0.1.50:8080
  poll:       every 30s
  model dir:  ./models

  [12:00:01] heartbeat ok — no model assigned
  [12:00:31] heartbeat ok — assigned defect-detector@v2.1.0
  [12:00:31] downloading defect-detector@v2.1.0 (2.1 MB)...
  [12:00:32] staging... done (8ms)
  [12:00:32] verifying... ok (sha-256 match)
  [12:00:32] applying... done (3ms)
  [12:00:32] health check... passed (29μs, 10 outputs)
  [12:00:32] ✓ model active: defect-detector@v2.1.0
  [12:01:02] heartbeat ok — model current
```

### Control plane observability

The control plane logs every heartbeat, model upload, and assignment change. The existing `FleetStatusSummary` gains model version info so `GET /api/v1/fleets/:id/status` shows rollout progress:

```json
{
    "fleet_id": "factory",
    "total_devices": 20,
    "online": 18,
    "offline": 2,
    "on_target_version": 15,
    "updating": 3,
    "failed": 0
}
```

---

## 15. Testing Strategy

### Unit tests (~15 new tests)

| Module | Tests |
|--------|-------|
| `ControlPlaneModelStore` | open, store, get_bytes, get_meta, list, persistence, duplicate version handling |
| `HeartbeatRequest/Response` | serialization roundtrip, backward compat (empty body) |
| `AgentClient` | (mock server) heartbeat, download, error handling |
| `RejectedVersions` | increment, max attempts, reset on new version |

### Integration tests (~10 new tests)

| Test | What it exercises |
|------|-------------------|
| `test_model_upload_and_download` | Upload via API → download via API → verify bytes match |
| `test_heartbeat_with_assignment` | Register device → assign model → heartbeat → verify response contains assignment |
| `test_fleet_deploy_sets_assignments` | Create fleet → deploy → verify all devices have assigned_model set |
| `test_agent_single_poll_cycle` | Start control plane in-process → run one agent poll → verify model downloaded and applied |
| `test_agent_rollback_on_bad_model` | Upload invalid ONNX → agent downloads → health check fails → verify rollback |
| `test_agent_no_retry_after_max_failures` | Upload bad model 3× → verify agent stops retrying that version |
| `test_heartbeat_backward_compat` | Empty body heartbeat still returns 200 with `{"status": "ok"}` |
| `test_model_upload_large` | Upload 10 MB model → download → verify integrity |
| `test_concurrent_agent_heartbeats` | 20 agents heartbeating simultaneously → no data races |
| `test_deploy_then_agent_picks_up` | Full E2E: serve → register → fleet → deploy → agent poll → model active |

### Stress tests (~3 new tests)

| Test | What it exercises |
|------|-------------------|
| `test_100_device_fleet_assignment` | Deploy to 100-device fleet → verify all 100 devices get assignments |
| `test_agent_survives_server_restart` | Agent running → kill server → restart server → agent recovers |
| `test_sequential_version_upgrades_via_agent` | Deploy v1 → agent picks up → deploy v2 → agent picks up → ... × 10 |

### E2E / Docker tests

```bash
# docker-compose.yml with two services:
#   control-plane: oxide serve --port 8080
#   agent: oxide agent --control-plane http://control-plane:8080 --device-id test-01

# Test script:
# 1. Wait for control plane healthy
# 2. Register device via API
# 3. Create fleet, add device
# 4. Upload model, deploy to fleet
# 5. Wait for agent to pick up (poll logs)
# 6. Verify model is active on agent (check state file or agent logs)
# 7. Upload new version, deploy again
# 8. Verify agent picks up new version
```

---

## 16. Deployment & Operations

### systemd service (recommended for Linux devices)

```ini
# /etc/systemd/system/oxide-agent.service
[Unit]
Description=Oxide Edge AI Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=oxide
ExecStart=/usr/local/bin/oxide agent \
    --control-plane http://10.0.1.50:8080 \
    --device-id %H \
    --model-dir /var/lib/oxide/models
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

### Docker (for demos and CI)

```dockerfile
FROM debian:bookworm-slim
COPY --from=builder /build/target/release/oxide /usr/local/bin/oxide
RUN useradd --create-home oxide
USER oxide
WORKDIR /home/oxide
RUN mkdir -p models
ENTRYPOINT ["oxide", "agent"]
CMD ["--control-plane", "http://control-plane:8080", "--device-id", "demo-01", "--poll-interval", "10"]
```

### docker-compose.yml (full demo)

```yaml
version: "3.8"
services:
  control-plane:
    build: .
    entrypoint: ["oxide", "serve", "--port", "8080"]
    ports:
      - "8080:8080"
    volumes:
      - cp-data:/home/oxide/.oxide

  agent-01:
    build: .
    entrypoint: ["oxide", "agent"]
    command:
      - "--control-plane"
      - "http://control-plane:8080"
      - "--device-id"
      - "agent-01"
      - "--poll-interval"
      - "10"
    depends_on:
      - control-plane

  agent-02:
    build: .
    entrypoint: ["oxide", "agent"]
    command:
      - "--control-plane"
      - "http://control-plane:8080"
      - "--device-id"
      - "agent-02"
      - "--poll-interval"
      - "10"
    depends_on:
      - control-plane

volumes:
  cp-data:
```

---

## 17. Known Limitations (Beta)

These are intentional scope cuts for internal beta. Each has a clear path to resolution.

| # | Limitation | Impact | Resolution Path |
|---|-----------|--------|-----------------|
| L1 | No authentication or TLS | Must run on trusted network | Add TLS + mTLS (rustls already a dep) |
| L2 | Pull-only, no push notifications | Worst-case latency = poll_interval before device sees update | Add optional WebSocket for instant notification |
| L3 | Full model download every time | Wastes bandwidth for large models with small changes | Add delta/diff updates |
| L4 | Single-model-per-device | Can't assign multiple models to one device | Extend assignment to a list |
| L5 | No rollout stage progression | Canary deploys set the first stage but don't auto-advance | Add stage advancement in fleet manager |
| L6 | JSON file persistence on control plane | Not suitable for >1000 devices | Replace with SQLite |
| L7 | No model provenance/signing | Can't verify who uploaded a model | Add ed25519 signatures |
| L8 | Agent doesn't expose inference | Application must load model file independently | Add optional local inference HTTP endpoint |
| L9 | Metrics are best-effort in heartbeat | No historical metrics storage | Add Prometheus endpoint + time-series storage |
| L10 | No graceful canary promotion | Must manually deploy at each stage | Add auto-promotion based on health check results |

---

## 18. Implementation Plan

Five steps, each independently testable and committable. Estimated ~5 working days.

### Step 1: Model Store on Control Plane (Day 1)

**What:** `ControlPlaneModelStore` + upload/download/meta API endpoints.

**Files:**
- `oxide-control/src/model_store.rs` (new, ~150 lines)
- `oxide-control/src/server.rs` (add 3 endpoints, ~80 lines)
- `oxide-control/src/lib.rs` (export)

**Tests:**
- Unit: store/get/list/persistence
- Integration: upload via HTTP → download via HTTP → bytes match

**Exit criteria:** Can `curl -X POST` a model file to the control plane and `curl -O` it back with correct SHA-256.

### Step 2: Extended Heartbeat (Day 2)

**What:** Heartbeat accepts a body with device state, returns model assignments.

**Files:**
- `oxide-core/src/device.rs` (add fields + types, ~40 lines)
- `oxide-control/src/server.rs` (modify heartbeat handler, ~30 lines)

**Tests:**
- Unit: serialization roundtrip
- Integration: heartbeat with body, verify response, backward compat with empty body

**Exit criteria:** `curl -X POST -d '{"current_model": null, "status": "online"}' /api/v1/devices/cam-01/heartbeat` returns assigned model.

### Step 3: Fleet Deploy with Upload + Assignment (Day 2–3)

**What:** `oxide deploy --fleet --control-plane` uploads model and sets assignments. `POST /api/v1/fleets/:id/deploy` sets `assigned_model` on all fleet devices.

**Files:**
- `oxide-control/src/fleet_manager.rs` (deploy sets assignments, ~20 lines)
- `oxide-cli/src/commands/deploy.rs` (add --control-plane path, ~60 lines)
- `oxide-cli/src/main.rs` (add --control-plane and --version flags to Deploy)

**Tests:**
- Integration: deploy → verify all fleet devices have correct assigned_model
- CLI: run deploy command against in-process server

**Exit criteria:** `oxide deploy model.onnx --fleet factory --control-plane http://localhost:8080` uploads model and all fleet devices show the assignment.

### Step 4: Agent Loop (Day 3–4)

**What:** `oxide agent` subcommand with poll loop, HTTP client, OTA integration.

**Files:**
- `oxide-network/src/client.rs` (new, ~120 lines)
- `oxide-cli/src/commands/agent.rs` (new, ~200 lines)
- `oxide-cli/src/main.rs` (add Agent variant, ~15 lines)

**Tests:**
- Integration: start control plane → run single agent poll cycle → verify model downloaded + applied
- Stress: agent survives server restart, sequential version upgrades
- Error: bad model → rollback → report failure

**Exit criteria:** Agent running in one terminal, deploy in another terminal, agent picks up the model within one poll interval.

### Step 5: Docker Demo + E2E Tests (Day 5)

**What:** docker-compose with control plane + 2 agents. E2E test script. Documentation updates.

**Files:**
- `docker-compose.yml` (new)
- `tests/e2e_agent.sh` (new)
- `README.md` (add agent section)
- `docs/design/agent-design.md` (mark as implemented, update with any deviations)

**Tests:**
- Full E2E: docker-compose up → register → fleet → deploy → agents pick up → deploy v2 → agents update

**Exit criteria:** `docker-compose up` + a test script demonstrates the full flow with zero manual steps.

---

## 19. Future Work

Ordered by priority for the release following internal beta.

| Priority | Feature | Effort |
|:--------:|---------|:------:|
| P0 | TLS + mTLS (transport security) | 2 days |
| P0 | API token auth (device identity) | 1 day |
| P1 | Canary stage auto-advancement | 2 days |
| P1 | Agent-hosted local inference HTTP endpoint | 1 day |
| P1 | Prometheus metrics endpoint on control plane | 1 day |
| P2 | SQLite persistence (replace JSON files) | 2 days |
| P2 | Model provenance signing (ed25519) | 2 days |
| P2 | Multi-model-per-device support | 1 day |
| P3 | WebSocket push notifications (optional, alongside pull) | 3 days |
| P3 | Delta model updates | 5 days |
| P3 | Python SDK for deployment scripting | 3 days |
| P3 | Kubernetes operator | 5 days |

---

## Appendix A: Full API Surface (after agent feature)

### Existing endpoints (unchanged)

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/health` | Control plane health |
| `GET` | `/api/v1/devices` | List all devices |
| `POST` | `/api/v1/devices` | Register device |
| `GET` | `/api/v1/devices/:id` | Get device |
| `DELETE` | `/api/v1/devices/:id` | Unregister device |
| `GET` | `/api/v1/fleets` | List fleets |
| `POST` | `/api/v1/fleets` | Create fleet |
| `GET` | `/api/v1/fleets/:id` | Get fleet |
| `POST` | `/api/v1/fleets/:id/devices/:did` | Add device to fleet |
| `GET` | `/api/v1/fleets/:id/status` | Fleet health summary |

### Modified endpoints

| Method | Path | Change |
|--------|------|--------|
| `POST` | `/api/v1/devices/:id/heartbeat` | Accepts body, returns assignments |
| `POST` | `/api/v1/fleets/:id/deploy` | Uploads model (if --control-plane), sets device assignments |

### New endpoints

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/api/v1/models/:id/versions/:ver` | Upload model bytes |
| `GET` | `/api/v1/models/:id/versions/:ver/download` | Download model bytes |
| `GET` | `/api/v1/models/:id/versions/:ver/meta` | Model metadata |
| `GET` | `/api/v1/models/:id` | List versions of a model |

---

## Appendix B: End-to-End Demo Script

```bash
#!/bin/bash
set -euo pipefail

# === Terminal 1: Control Plane ===
oxide serve --port 8080 &
CP_PID=$!
sleep 1

# === Setup: Register devices and fleet ===
curl -s -X POST localhost:8080/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{"id": "cam-01", "name": "East Camera"}'

curl -s -X POST localhost:8080/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{"id": "cam-02", "name": "West Camera"}'

curl -s -X POST localhost:8080/api/v1/fleets \
  -H "Content-Type: application/json" \
  -d '{"id": "factory", "name": "Factory Floor"}'

curl -s -X POST localhost:8080/api/v1/fleets/factory/devices/cam-01
curl -s -X POST localhost:8080/api/v1/fleets/factory/devices/cam-02

# === Deploy model ===
oxide deploy models/test/classifier_model.onnx \
  --fleet factory \
  --version v1.0.0 \
  --control-plane http://localhost:8080

# === Terminal 2: Agent (simulating cam-01) ===
oxide agent \
  --control-plane http://localhost:8080 \
  --device-id cam-01 \
  --poll-interval 5 \
  --model-dir /tmp/oxide-cam-01 &
AGENT_PID=$!

# Wait for agent to pick up model
sleep 15

# Verify
cat /tmp/oxide-cam-01/.agent-state.json
# → {"current_model": "classifier_model", "current_model_version": "v1.0.0", ...}

# === Deploy v2 ===
oxide deploy models/test/mlp_mnist.onnx \
  --fleet factory \
  --version v2.0.0 \
  --control-plane http://localhost:8080

# Wait for agent to pick up v2
sleep 15

cat /tmp/oxide-cam-01/.agent-state.json
# → {"current_model": "mlp_mnist", "current_model_version": "v2.0.0", ...}

# Cleanup
kill $AGENT_PID $CP_PID
```
