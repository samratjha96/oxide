# ⚡ Oxide

```
   ██████  ██   ██ ██ ██████  ███████
  ██    ██  ██ ██  ██ ██   ██ ██
  ██    ██   ███   ██ ██   ██ █████
  ██    ██  ██ ██  ██ ██   ██ ██
   ██████  ██   ██ ██ ██████  ███████
```

**ML model delivery for device fleets. Ships only the bytes that changed.**

Oxide delivers ML model updates to edge devices using the minimum possible bandwidth. It understands model file structure — tensors, weights, layers — and ships only what changed between versions.

One binary. Control plane + device agent. ONNX-aware delta compression with automatic fallback. Pull-based delivery (works through NATs and firewalls). Rollback on failure.

<p align="center">
  <img alt="License" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue" />
  <img alt="Binary" src="https://img.shields.io/badge/binary-6.0%20MB-green" />
  <img alt="Delta" src="https://img.shields.io/badge/delta-99.9%25%20savings-blueviolet" />
</p>

---

## The Problem

A 100 MB model pushed to 1,000 devices = 100 GB of bandwidth per update. Over LTE Cat-M1 (~300 kbps), that's 45 minutes per device and $5–30/GB in cellular data costs.

But fine-tuning typically changes <20% of weights. 80%+ of that transfer is redundant bytes the device already has.

**Oxide computes tensor-level deltas**: it parses your ONNX file, hashes each tensor, and ships only the changed ones with XOR compression. For transfer learning (last layer only), a 2.1 MB model update becomes **1,263 bytes**.

---

## How It Works

```
Upload v2                                    Agent has v1
┌──────────────┐                          ┌──────────────┐
│ Control Plane│                          │ Device Agent  │
│              │                          │               │
│ Upload v2 ───┼→ compute delta(v1, v2)   │ heartbeat ────┼→ "I have v1"
│              │  tensor XOR: 1,263 bytes  │               │
│              │                          │ ← "here's v2" │
│              │  ─── delta patch ────→   │               │
│              │  (instead of 2.1 MB)     │ reconstruct   │
│              │                          │ verify SHA-256│
│              │                          │ apply + check │
└──────────────┘                          └──────────────┘
```

### Two delta strategies, best wins

| Scenario | Full file | Tensor delta | Binary delta | Winner |
|----------|----------:|-------------:|-------------:|--------|
| Transfer learning (last layer) | 2,094 KB | **9 KB** | 10 KB | Tensor |
| Fine-tuning (all layers, 5%) | 2,094 KB | 154 KB | **121 KB** | Binary |
| Full retrain | 2,094 KB | 1,981 KB | **1,942 KB** | Binary |

- **Tensor-level** (ML-aware): Parse ONNX protobuf, hash each tensor, skip unchanged, XOR-delta changed tensors
- **Binary** (format-agnostic): zstd dictionary compression using previous version as dictionary
- **Decision**: compute both, cache whichever is smaller. Fall back to full file if delta is larger.

### Measured E2E (real processes, real ONNX model)

```
Upload v1 (2.1 MB)    → stored
Upload v2 (last layer) → delta cached: 1,263 bytes (99.9% savings, Tensor strategy)

Agent picks up v1:
  downloading m1@v1... model ready: 2143752 bytes
  staging... done → verifying... ok → applying... done
  health check... passed (113μs)
  ✓ model active: m1@v1

Deploy v2 to fleet:
  downloading m1@v2...
  received delta (1263 bytes), reconstructing...
  reconstructed 2143752 bytes (99.9% bandwidth saved)
  staging... done → verifying... ok → applying... done
  health check... passed (59μs)
  ✓ model active: m1@v2
```

---

## Quick Start

```bash
git clone https://github.com/samratjha96/oxide && cd oxide
cargo build --release

# Start control plane
./target/release/oxide serve --port 8080

# In another terminal: register device + fleet
curl -X POST localhost:8080/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{"id": "cam-01", "name": "East Camera"}'

curl -X POST localhost:8080/api/v1/fleets \
  -H "Content-Type: application/json" \
  -d '{"id": "factory", "name": "Factory Floor"}'

curl -X POST localhost:8080/api/v1/fleets/factory/devices/cam-01

# Upload model versions
curl -X POST localhost:8080/api/v1/models/defect/versions/v1 \
  --data-binary @model-v1.onnx

curl -X POST localhost:8080/api/v1/models/defect/versions/v2 \
  --data-binary @model-v2.onnx
# → delta cached automatically on upload

# Create deployment campaign
curl -X POST localhost:8080/api/v1/campaigns \
  -H "Content-Type: application/json" \
  -d '{"model_id": "defect", "model_version": "v2", "fleet_id": "factory"}'

# Start agent on device (downloads v2 via delta)
./target/release/oxide agent \
  --control-plane http://10.0.0.1:8080 \
  --device-id cam-01 \
  --poll-interval 30
```

---

## Features

### Delta compression (oxide-delta crate)

The core differentiator. Pure Rust, no network dependency.

- **ONNX protobuf parser** — extracts tensor names, sizes, and raw bytes via prost
- **SafeTensors detection** — JSON header + flat tensor layout
- **Tensor manifest** — per-tensor SHA-256 hashes (sent in download headers)
- **OXDL patch format** — binary format with COPY/REPLACE/XOR chunks
- **Round-trip verified** — SHA-256 check on reconstructed file, always

### Control plane

- Model store with automatic delta caching on upload
- Delta-aware download endpoint (serves patch or full file based on headers)
- Device registry with heartbeat tracking
- Fleet management (create, add devices, deploy)
- Campaign tracking with per-device progress and bandwidth stats
- Campaign lifecycle: create → rolling_out → pause/resume → complete/abort

### Device agent

- Pull-based polling (works through NATs, firewalls, intermittent connectivity)
- Delta-aware downloads with automatic fallback to full file
- Full OTA pipeline: stage → verify → backup → apply → health check → rollback
- Poison pill protection (won't retry same broken version 3+ times)
- State persistence across restarts
- Graceful shutdown on SIGTERM/ctrl-c

### Also included

- ONNX inference via [tract](https://github.com/sonos/tract) (microsecond latency)
- AES-256-GCM model encryption
- Canary/rolling/all-at-once rollout strategies
- Benchmarking CLI

---

## API

```
Devices
  POST   /api/v1/devices                           Register
  GET    /api/v1/devices                           List
  GET    /api/v1/devices/:id                       Get
  DELETE /api/v1/devices/:id                       Remove
  POST   /api/v1/devices/:id/heartbeat             Heartbeat

Fleets
  POST   /api/v1/fleets                            Create
  GET    /api/v1/fleets/:id                        Get
  POST   /api/v1/fleets/:id/devices/:did           Add device
  POST   /api/v1/fleets/:id/deploy                 Deploy (legacy)
  GET    /api/v1/fleets/:id/status                 Status

Models
  POST   /api/v1/models/:id/versions/:ver          Upload (triggers delta computation)
  GET    /api/v1/models/:id/versions/:ver/download  Download (delta-aware)
  GET    /api/v1/models/:id/versions/:ver/meta      Metadata
  GET    /api/v1/models/:id                        List versions

Campaigns
  POST   /api/v1/campaigns                         Create campaign
  GET    /api/v1/campaigns                         List
  GET    /api/v1/campaigns/:id                     Status + per-device breakdown
  POST   /api/v1/campaigns/:id/pause               Pause
  POST   /api/v1/campaigns/:id/resume              Resume
  POST   /api/v1/campaigns/:id/abort               Abort

Health
  GET    /health                                    Control plane health
```

---

## Architecture

```
crates/
├── oxide-delta       ML-aware delta compression (ONNX parser, OXDL format)
├── oxide-core        Types, config, errors, metrics
├── oxide-models      ONNX loading via tract
├── oxide-runtime     Inference engine, model store, health checks
├── oxide-security    AES-256-GCM encryption, SHA-256 integrity
├── oxide-network     Device REST API, OTA update engine
├── oxide-control     Registry, fleet manager, model store, campaigns, server
└── oxide-cli         The binary (11 subcommands)
```

---

## Development

```bash
cargo build --workspace                  # Debug build
cargo build --release                    # Release (6 MB binary)
cargo test --workspace                   # All tests
cargo clippy --workspace --tests         # Zero warnings
```

---

## License

MIT or Apache 2.0, at your option.
