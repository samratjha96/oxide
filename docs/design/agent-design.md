# oxide agent: Device-Side Daemon Design

## The Problem

Today, `oxide deploy --fleet factory` updates a database saying "factory should run model v3." But nothing actually pushes the model to the devices. There's no process running on each device that knows it needs an update.

The missing piece: a daemon on each device that calls home, discovers it has a pending update, downloads the model, applies it through the existing OTA engine, and reports back.

## How It Works

```
┌─────────────────────────────────────┐
│         Control Plane               │
│  oxide serve --port 8080            │
│                                     │
│  ┌───────────┐  ┌───────────────┐   │
│  │  Device    │  │  Model        │   │
│  │  Registry  │  │  Store (disk) │   │
│  └───────────┘  └───────────────┘   │
│         ▲              │            │
│         │ heartbeat    │ model      │
│         │ + status     │ bytes      │
│         │              ▼            │
└─────────┼──────────────┼────────────┘
          │   HTTP(S)    │
          │              │
     ┌────┴──────────────┴────┐
     │    oxide agent          │
     │    (runs on each device)│
     │                         │
     │  1. Call home (poll)     │
     │  2. Check assignment    │
     │  3. Download model      │
     │  4. OTA: stage/verify/  │
     │     apply/health-check  │
     │  5. Report result       │
     │  6. Run inference       │
     └────────────────────────┘
```

### The agent loop

The agent is a single long-running process. It does one thing on a timer:

```
every <poll_interval> seconds:
  1. POST /api/v1/devices/{id}/heartbeat
     - sends: current model id + version, device status, basic metrics
     - receives: assigned model id + version (if any)
  
  2. if assigned model != current model:
     GET /api/v1/models/{model_id}/versions/{version}/download
     - receives: model bytes + sha256
  
  3. run existing OTA pipeline:
     stage -> verify sha256 -> backup current -> apply -> health check
  
  4. POST /api/v1/devices/{id}/report
     - sends: update result (success/failure), new model id + version
     - on failure: rollback already happened locally, report the error

between polls:
  - the inference engine is available for the application to call
  - (oxide agent does NOT do inference itself — it just keeps the model current)
```

### What changes on the control plane

The control plane needs three new capabilities:

**1. Model storage** — When someone runs `oxide deploy model.onnx --fleet factory`, the control plane must store the model bytes so devices can download them. Today the deploy command only records intent. It needs to also upload the model.

New endpoints:
```
POST   /api/v1/models/{id}/versions/{version}          Upload model (multipart)
GET    /api/v1/models/{id}/versions/{version}/download  Download model bytes
GET    /api/v1/models/{id}/versions/{version}/meta      Model metadata + sha256
```

Storage: just files on disk under `<data_dir>/models/<id>/<version>.onnx`. No database. The control plane is meant to run on a single machine (a server in the factory, a laptop on the same network), not as a distributed system.

**2. Device assignments** — When a fleet deploy happens, the control plane records "device X should be running model Y at version Z." The heartbeat response tells the device what it should be running.

New field in the device record:
```rust
pub struct Device {
    // ... existing fields ...
    pub assigned_model: Option<ModelId>,
    pub assigned_model_version: Option<ModelVersion>,
}
```

The heartbeat response becomes:
```json
{
  "status": "ok",
  "assigned_model": "defect-detector",
  "assigned_model_version": "v2.1.0"
}
```

The agent compares `assigned_model_version` to `current_model_version`. If they differ, it downloads and applies.

**3. Update reporting** — Devices report back whether the update succeeded or failed. The control plane updates the device record accordingly.

The existing `POST /api/v1/devices/{id}/heartbeat` gets extended. No new endpoint needed. The heartbeat payload becomes:

```json
{
  "current_model": "defect-detector",
  "current_model_version": "v2.1.0",
  "status": "online",
  "last_update_result": "success",
  "metrics": {
    "inference_count": 14302,
    "avg_latency_us": 31.2,
    "uptime_secs": 86400
  }
}
```

### What changes in the CLI

New subcommand:
```
oxide agent --control-plane http://10.0.1.50:8080 --device-id cam-01 --poll-interval 30
```

Flags:
- `--control-plane <url>` — where the control plane is running (required)
- `--device-id <id>` — this device's ID (required, must be pre-registered)
- `--poll-interval <secs>` — how often to check in (default: 30)
- `--model-dir <path>` — where to store models locally (default: `./models`)
- `--key <path>` — encryption key for encrypted model downloads (optional)

The agent command blocks forever. It's meant to be run as a systemd service, a Docker entrypoint, or just `oxide agent &` in a startup script.

### What changes in `oxide deploy`

Today:
```
oxide deploy model.onnx --fleet factory
```
This records intent in the fleet manager (in-memory or JSON).

After this feature:
```
oxide deploy model.onnx --fleet factory --control-plane http://10.0.1.50:8080
```
This:
1. Uploads the model bytes to the control plane's model store
2. Sets the fleet's target model + version
3. Updates every device in the fleet's `assigned_model` + `assigned_model_version`

The devices pick it up on their next heartbeat. No push. Pure pull.

## What we're NOT building

- **Push-based delivery.** Devices pull on their schedule. No websockets, no MQTT, no bidirectional channels. Pull is simpler, works through NATs and firewalls, and handles intermittent connectivity naturally — the device just polls when it's online.

- **Model diffing or delta updates.** The device downloads the full model every time. Our models are 1KB–2MB. Even a 100MB model is a single HTTP GET. Delta updates are a premature optimization that adds massive complexity.

- **mTLS.** Not in this iteration. The agent connects over plain HTTP. Adding `--tls-cert` / `--tls-key` flags and wiring in rustls is a follow-up. The plumbing (rustls is already a dependency) is there; the configuration isn't.

- **Model format conversion or optimization on the control plane.** You upload an ONNX file, devices download the same ONNX file. If you want quantization, do it before upload.

- **Multi-tenancy or auth.** One control plane serves one deployment. No API keys, no user accounts, no RBAC. This is for a team deploying to their own devices, not a SaaS platform.

## Code changes by crate

### oxide-core
- Add `assigned_model` and `assigned_model_version` fields to `Device`
- Add `HeartbeatRequest` and `HeartbeatResponse` types
- Add `UpdateReport` type

### oxide-control
- `server.rs`: New model upload/download endpoints. Extended heartbeat handler that accepts a body and returns assignments. New fleet deploy handler that stores model bytes and sets assignments.
- `model_store.rs` (new): Filesystem-based model store. `store(id, version, bytes)`, `get(id, version) -> bytes`, `meta(id, version) -> sha256 + size`.

### oxide-network
- `client.rs` (new): HTTP client for the agent to talk to the control plane. `heartbeat()`, `download_model()`, `report_update()`. Uses `reqwest` (already common in Rust, but we might use `ureq` to avoid pulling in a full async HTTP client — the agent's HTTP needs are simple).

### oxide-cli
- `commands/agent.rs` (new): The agent loop. Parses config, starts the poll loop, calls the network client, drives the OTA engine.
- `main.rs`: Add `Agent` variant to the `Commands` enum.

### New dependencies
- `ureq` (synchronous HTTP client, ~200KB, pure Rust, no async runtime needed for the agent's simple poll loop). Alternatively, reuse the tokio + reqwest we already have — but ureq keeps the agent simpler since it doesn't need async.

## The demo

After implementation, this is the full end-to-end:

**Terminal 1 — Control plane (on a server or laptop):**
```bash
oxide serve --port 8080
```

**Terminal 2 — Register devices and deploy:**
```bash
# Register devices
oxide device register cam-01 --name "East Camera"
oxide device register cam-02 --name "West Camera"

# Create fleet
oxide fleet create factory --name "Factory Floor"

# Deploy model to fleet (uploads model + sets assignments)
oxide deploy defect-model-v3.onnx \
  --fleet factory \
  --control-plane http://localhost:8080
```

**Terminal 3 — Device 1 (on a Raspberry Pi or in a Docker container):**
```bash
oxide agent \
  --control-plane http://10.0.1.50:8080 \
  --device-id cam-01 \
  --poll-interval 10

# Output:
# oxide agent
#   device:     cam-01
#   control:    http://10.0.1.50:8080
#   poll:       every 10s
#   model dir:  ./models
#
#   [00:00:01] heartbeat ok — no model assigned
#   [00:00:11] heartbeat ok — assigned defect-model@v3.0.0
#   [00:00:11] downloading defect-model@v3.0.0 (2.1 MB)...
#   [00:00:12] staging... done (8ms)
#   [00:00:12] verifying... ok (sha-256 match)
#   [00:00:12] applying... done (3ms)
#   [00:00:12] health check... passed (29us, 10 outputs)
#   [00:00:12] model active: defect-model@v3.0.0
#   [00:00:22] heartbeat ok — model current
#   [00:00:32] heartbeat ok — model current
```

**Terminal 4 — Push a new version:**
```bash
oxide deploy defect-model-v4.onnx \
  --fleet factory \
  --rollout canary \
  --control-plane http://localhost:8080
```

On the device, the next heartbeat picks up the new assignment and applies it.

## Implementation order

1. **Model store** on the control plane (filesystem, upload/download endpoints) — smallest useful unit, testable immediately.

2. **Extended heartbeat** — add request body, return assignments. This is a backward-compatible change to an existing endpoint.

3. **Fleet deploy with model upload** — wire `oxide deploy --control-plane` to upload model bytes and set device assignments.

4. **Agent loop** — the daemon that polls, downloads, applies. Uses existing OTA engine, existing inference engine for health checks.

5. **Docker demo** — two containers (control plane + agent), show the full flow.

Each step is independently testable and committable. No step depends on a later step to be useful.

## Open questions (decisions to make during implementation)

1. **Model versioning** — When someone runs `oxide deploy model.onnx --fleet factory`, what's the version? Options: (a) auto-generate from sha256 prefix, (b) require `--version v3.0.0` flag, (c) derive from filename. Leaning toward (b) with (a) as default.

2. **Agent as blocking or async** — The agent poll loop is fundamentally synchronous (sleep → poll → maybe download → sleep). Using `ureq` (blocking) keeps it simple. Using tokio + reqwest reuses existing deps but adds complexity. Leaning toward tokio since it's already in the binary and the control plane server needs it anyway.

3. **How the agent exposes inference** — The agent keeps the model current, but who calls inference? Options: (a) the application links against oxide-runtime as a library, (b) the agent exposes a local HTTP endpoint for inference, (c) the agent just manages the model file and the application loads it independently. Leaning toward (a) for tight integration, (c) for simplicity. Start with (c) — the agent writes the model to a known path, the application picks it up.

4. **Config file vs flags** — The agent has enough settings that a config file makes sense. But we already have `oxide.toml`. Add an `[agent]` section there and let `oxide agent` read it. Flags override config.
