# Oxide

```
   ██████  ██   ██ ██ ██████  ███████
  ██    ██  ██ ██  ██ ██   ██ ██
  ██    ██   ███   ██ ██   ██ █████
  ██    ██  ██ ██  ██ ██   ██ ██
   ██████  ██   ██ ██ ██████  ███████
```

Oxide ships ML model updates to fleets of edge devices and sends only the bytes that changed.

It is a single Rust binary that runs as either the control plane or the device agent. The control plane stores model versions and computes deltas between them. The agent polls for updates, downloads a delta, rebuilds the new model from the one it already has, and rolls back if the health check fails.

## Why

A 100 MB model sent to 1,000 devices is 100 GB per update. Over an LTE Cat-M1 link (about 300 kbps) that takes roughly 45 minutes per device, and cellular data costs $5 to $30 per GB.

Most updates don't touch most of the weights. If you only retrain the last layer, everything else is already on the device. Oxide parses the ONNX file, hashes each tensor, and sends only the tensors that differ.

## How it works

When you upload a new version, the control plane computes a delta from each earlier version using two strategies and keeps the smaller one:

- **Tensor delta.** Parse the ONNX protobuf and compare tensors by SHA-256. Unchanged tensors are skipped. Changed tensors are XORed against the old version and compressed with zstd. The graph structure is sent separately with the weights stripped out.
- **Binary delta.** Compress the new file with zstd, using the old file as the dictionary. This works for any format.

If neither delta is smaller than the full file, the full file is sent instead.

On the device, the agent:

1. Sends a heartbeat with the version it has.
2. Downloads the delta for its assigned version, or the full file if there is no delta.
3. Rebuilds the model and checks its SHA-256.
4. Backs up the current model and applies the new one.
5. Runs the health check and rolls back if it fails.

## Benchmarks

Delta size for a 2.1 MB ONNX model:

| Change | Full file | Tensor delta | Binary delta |
|---|---:|---:|---:|
| Last layer retrained | 2,094 KB | **9 KB** | 10 KB |
| 5% of weights changed in every layer | 2,094 KB | 154 KB | **121 KB** |
| Full retrain | 2,094 KB | 1,981 KB | **1,942 KB** |

An end-to-end run with a real control plane and agent turned a last-layer update into a 1,263-byte download:

```
Upload v1 (2.1 MB)     → stored
Upload v2 (last layer) → delta cached: 1,263 bytes (Tensor strategy)

Agent on v1, deploy v2:
  received delta (1263 bytes), reconstructing...
  reconstructed 2143752 bytes
  staging... done → verifying... ok → applying... done
  health check... passed (59μs)
  model active: m1@v2
```

Run it yourself with `scripts/demo.sh`.

## Quick start

```bash
git clone https://github.com/samratjha96/oxide && cd oxide
cargo build --release

# Start the control plane
./target/release/oxide serve --port 8080

# Register a device and a fleet
curl -X POST localhost:8080/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{"id": "cam-01", "name": "East Camera"}'

curl -X POST localhost:8080/api/v1/fleets \
  -H "Content-Type: application/json" \
  -d '{"id": "factory", "name": "Factory Floor"}'

curl -X POST localhost:8080/api/v1/fleets/factory/devices/cam-01

# Upload two versions. The delta is computed on upload.
curl -X POST localhost:8080/api/v1/models/defect/versions/v1 \
  --data-binary @model-v1.onnx

curl -X POST localhost:8080/api/v1/models/defect/versions/v2 \
  --data-binary @model-v2.onnx

# Roll v2 out to the fleet
curl -X POST localhost:8080/api/v1/campaigns \
  -H "Content-Type: application/json" \
  -d '{"model_id": "defect", "model_version": "v2", "fleet_id": "factory"}'

# Start the agent on the device
./target/release/oxide agent \
  --control-plane http://10.0.0.1:8080 \
  --device-id cam-01 \
  --poll-interval 30
```

## Features

Control plane:

- Stores model versions and computes deltas on upload
- Serves a delta or the full file depending on what the device already has
- Tracks devices by heartbeat and groups them into fleets
- Runs rollout campaigns you can pause, resume, or abort, with per-device progress and bandwidth saved

Agent:

- Polls for updates, so it works behind NAT and firewalls
- Falls back to a full download if a delta can't be applied
- Rolls back when the health check fails, and stops retrying a version after 3 failures
- Keeps its state across restarts

Also included: ONNX inference via [tract](https://github.com/sonos/tract), AES-256-GCM model encryption, canary and rolling rollouts, and a benchmark command.

## API

```
Devices
  POST   /api/v1/devices                            Register
  GET    /api/v1/devices                            List
  GET    /api/v1/devices/:id                        Get
  DELETE /api/v1/devices/:id                        Remove
  POST   /api/v1/devices/:id/heartbeat              Heartbeat

Fleets
  POST   /api/v1/fleets                             Create
  GET    /api/v1/fleets/:id                         Get
  POST   /api/v1/fleets/:id/devices/:did            Add device
  POST   /api/v1/fleets/:id/deploy                  Deploy (legacy)
  GET    /api/v1/fleets/:id/status                  Status

Models
  POST   /api/v1/models/:id/versions/:ver           Upload
  GET    /api/v1/models/:id/versions/:ver/download  Download (delta if available)
  GET    /api/v1/models/:id/versions/:ver/meta      Metadata
  GET    /api/v1/models/:id                         List versions

Campaigns
  POST   /api/v1/campaigns                          Create
  GET    /api/v1/campaigns                          List
  GET    /api/v1/campaigns/:id                      Status and per-device breakdown
  POST   /api/v1/campaigns/:id/pause                Pause
  POST   /api/v1/campaigns/:id/resume               Resume
  POST   /api/v1/campaigns/:id/abort                Abort

Health
  GET    /health                                    Control plane health
```

## Layout

```
crates/
├── oxide-delta      Delta compression (ONNX parser, OXDL patch format)
├── oxide-core       Shared types, config, errors, metrics
├── oxide-models     ONNX loading via tract
├── oxide-runtime    Inference engine, model store, health checks
├── oxide-security   AES-256-GCM encryption, SHA-256 checks
├── oxide-network    Device API, OTA update pipeline
├── oxide-control    Registry, fleets, model store, campaigns, HTTP server
└── oxide-cli        The oxide binary
```

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --tests
```

## License

MIT or Apache 2.0, at your option.
