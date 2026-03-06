# ⚡ Oxide

**You trained the model. Now ship it to 1,000 devices without losing your mind.**

```bash
# You have an ONNX model. You have a Raspberry Pi. Go.
oxide run defect-detector.onnx --input image.json --shape "1,3,224,224"
# ✓ Loaded in 3ms · Inference: 29μs · Output: [0.02, 0.97, 0.01]
```

Oxide is an edge AI runtime that replaces your Python deployment scripts, your SSH-into-every-device workflow, and your "it works on my laptop" prayers with a single 6 MB binary.

Load ONNX models. Run inference in microseconds. Push updates to your entire fleet. Roll back when things go wrong. Encrypt models so competitors can't steal them off your devices. All from one CLI.

<br />

<p align="center">
  <img alt="License" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue" />
  <img alt="Binary" src="https://img.shields.io/badge/binary-6.0%20MB-green" />
  <img alt="Latency" src="https://img.shields.io/badge/P50-29μs-blueviolet" />
</p>

---

## Who this is for

**ML engineers** who've trained a model in PyTorch and now need it running on devices that aren't their laptop. You export to ONNX, you hand it to Oxide, it runs. No Python. No Docker. No JVM. No "install these 47 system packages."

**Embedded / IoT teams** managing fleets of cameras, sensors, drones, or robots. You need to push a new model to 500 devices on Monday morning without taking down production. You need rollback when the new model is worse. You need to know it actually deployed.

**Edge infrastructure engineers** building the deployment layer between "data science says the model is ready" and "it's running in the factory." You're tired of gluing together SCP scripts, systemd units, and hope.

If you're running models on devices with 1–8 GB of RAM, intermittent connectivity, and no patience for 500 MB runtimes, this is your tool.

---

## 60 seconds to inference

```bash
git clone https://github.com/oxide-ai/oxide && cd oxide
cargo build --release          # 6 MB binary

# See what's inside your model
./target/release/oxide info your-model.onnx
# ⚡ Oxide — Model Info
#   Format:   ONNX
#   Size:     2,093 KB
#   Inputs:   input [1, 784] (F32)
#   Outputs:  softmax [1, 10] (F32)

# Run it
./target/release/oxide run your-model.onnx
# ✓ Loaded in 3.2ms
# 🔥 Inference: 29μs
# Output: [0.01, 0.02, 0.91, 0.01, ...]

# Benchmark it properly
./target/release/oxide bench your-model.onnx --warmup 100 --iterations 5000
# ────────────────────────────
# P50:         29.4 μs
# P99:         31.6 μs
# Throughput:  33,870 inferences/sec
# ────────────────────────────
```

That's a 535,000-parameter MLP. Loading + first inference in under 5 ms. On CPU. No GPU. A 6 MB binary.

---

## What it actually does

### 1. Load and run ONNX models — fast

Oxide uses [tract](https://github.com/sonos/tract), a pure-Rust inference engine. No C library to cross-compile. No shared objects to chase down. It detects your CPU and auto-enables NEON, AMX, or AVX acceleration.

```bash
oxide run face-detector.onnx --input "[0.5, 0.3, ...]" --shape "1,3,224,224"
```

| Model | Params | P50 Latency | Throughput |
|-------|-------:|------------:|-----------:|
| Sigmoid (trivial) | 0 | 0.96 μs | 1,065,424 /s |
| Classifier (FC→ReLU→Softmax) | 736 | 4.6 μs | 219,449 /s |
| MLP-MNIST (784→512→256→10) | 535K | 29 μs | 33,870 /s |

### 2. Deploy to devices with integrity checks and rollback

`oxide deploy` doesn't just copy a file. It stages the model, verifies its SHA-256 hash, backs up the current version, applies the update atomically, then loads the new model and runs a health check. If anything fails, the old model comes back.

```bash
oxide deploy defect-model.onnx --device rpi-cam-01

# 📦 Staging model...        done (509 μs)
# 🔍 Verifying integrity...  ✓ SHA-256 match
# 🚀 Applying update...      done (5.2 ms)
# 💚 Health check...         ✓ passed (3.7 ms, 4 outputs)
# ✅ Deployed to 'rpi-cam-01'
```

If the health check fails, the previous model is restored automatically. No manual intervention. No "it's stuck on the broken version until someone SSHes in."

### 3. Manage fleets — not individual devices

Register devices once. Group them into fleets. Deploy to the fleet.

```bash
# Register your devices
oxide device register cam-01 --name "Assembly Line East"
oxide device register cam-02 --name "Assembly Line West"
oxide device register cam-03 --name "Loading Dock"

# Group them
oxide fleet create factory --name "Factory Floor"

# Deploy with canary rollout
oxide deploy new-model.onnx --fleet factory --rollout canary
```

Rollout strategies:

| Strategy | What happens |
|----------|-------------|
| `all_at_once` | Every device, right now. |
| `canary` | 5% → 25% → 50% → 100%, with health checks between stages. |
| `rolling` | N devices at a time, sequential batches. |

### 4. Encrypt models so they can't be stolen

Your model is your IP. If someone pulls the SD card out of a device in the field, they shouldn't get your model.

```bash
# Encrypt before shipping
oxide encrypt proprietary-model.onnx --key production.key
# → proprietary-model.onnx.enc (AES-256-GCM)

# Decrypt on the device before loading
oxide decrypt proprietary-model.onnx.enc --key production.key
```

AES-256-GCM provides both confidentiality and tamper detection. If a single bit of the encrypted file is modified, decryption fails — not silently, not with garbage output, it fails.

### 5. Run a control plane

For larger deployments, `oxide serve` starts an HTTP API that manages devices, fleets, model storage, and deployments programmatically. Graceful shutdown on SIGTERM/ctrl-c.

```bash
oxide serve --port 8080

# Register a device
curl -X POST localhost:8080/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{"id": "cam-01", "name": "East Camera"}'

# Upload a model
curl -X POST localhost:8080/api/v1/models/defect-v7/versions/v7.2.0 \
  --data-binary @defect-model.onnx

# Deploy to a fleet (assigns model to all fleet devices)
curl -X POST localhost:8080/api/v1/fleets/factory/deploy \
  -H "Content-Type: application/json" \
  -d '{"model_id": "defect-v7", "model_version": "v7.2.0", "strategy": "canary"}'
# → {"status":"deployed","total_devices":20,"successful":20,"failed":0}
```

Full API:

```
POST   /api/v1/devices                           Register device
GET    /api/v1/devices                           List devices
GET    /api/v1/devices/:id                       Get device
DELETE /api/v1/devices/:id                       Remove device
POST   /api/v1/devices/:id/heartbeat             Heartbeat (device → CP)

POST   /api/v1/fleets                           Create fleet
GET    /api/v1/fleets/:id                       Get fleet
POST   /api/v1/fleets/:id/devices/:did           Add device to fleet
POST   /api/v1/fleets/:id/deploy                Deploy to fleet
GET    /api/v1/fleets/:id/status                Fleet health summary

POST   /api/v1/models/:id/versions/:ver          Upload model binary
GET    /api/v1/models/:id/versions/:ver/download  Download model binary
GET    /api/v1/models/:id/versions/:ver/meta      Model metadata (size, SHA-256)
GET    /api/v1/models/:id                        List versions

GET    /health                                   Control plane health
```

### 6. Run an agent daemon on each device

`oxide agent` is a long-running daemon that polls the control plane for model assignments and applies updates via OTA — no SSH, no manual intervention.

```bash
oxide agent \
  --control-plane http://10.0.0.1:8080 \
  --device-id cam-01 \
  --poll-interval 30 \
  --model-dir /opt/oxide/models
```

The agent:
- **Polls on a timer** — sends a heartbeat with device state and metrics, gets back any pending model assignment
- **Full OTA pipeline** — stage → SHA-256 verify → backup current → apply → load model → health check (live inference)
- **Automatic rollback** — if the health check fails, the previous model is restored
- **Poison pill protection** — won't retry the same broken model+version more than 3 times
- **State persistence** — survives restarts; picks up where it left off
- **Graceful shutdown** — saves state on SIGTERM/ctrl-c

Works through NATs and firewalls (pull-based, outbound HTTP only). Handles intermittent connectivity with exponential backoff.

---

## Why not just use...

**Python + Flask on each device?**
500 MB runtime. 5-second startup. GC pauses during inference. Cross-compiling Python to ARM is its own job. Oxide is a single 6 MB static binary with microsecond inference.

**TensorFlow Lite?**
Good inference engine. Zero fleet management. Zero OTA. Zero encryption. You still need to build everything around it. Oxide is the deployment layer TF Lite doesn't have.

**AWS IoT Greengrass / Azure IoT Edge?**
200 MB+ agent. Cloud lock-in. Per-device pricing. Works great until your devices go offline. Oxide is open source, offline-first, and runs without a cloud account.

**Writing your own bash scripts + SCP?**
It works until device 47 has a different OS version, device 112 runs out of disk during the copy, and device 203 is offline. Oxide handles staging, verification, atomic apply, health checks, and rollback — so you don't.

---

## Performance

Measured on Apple M4 Pro. Release build. LTO enabled, symbols stripped.

| What | Number |
|------|-------:|
| Binary size | **6.0 MB** |
| Model load (535K params) | **3.2 ms** |
| P50 inference (535K params) | **29 μs** |
| P99 inference (535K params) | **32 μs** |
| Throughput (535K params) | **33,870 /s** |
| Cold start to first inference | **< 5 ms** |
| OTA deploy (stage + verify + apply + health) | **< 10 ms** |

All targets met:

| | Goal | Actual |
|---|---|---:|
| Binary | < 10 MB | 6.0 MB ✅ |
| Cold start | < 1 s | 3.2 ms ✅ |
| Inference | < 10 ms | 29 μs ✅ |
| Memory | < 50 MB | < 8 MB ✅ |

---

## Architecture

Seven crates. Pure Rust. Zero C dependencies. The inference engine, crypto, HTTP server, and CLI are all native — no OpenSSL, no libtensorflow, no libonnxruntime.

```
crates/
├── oxide-core        Types, config, errors, metrics primitives
├── oxide-models      ONNX loading + inference via tract
├── oxide-runtime     Inference engine, model store, health checks
├── oxide-security    AES-256-GCM encryption, SHA-256 integrity
├── oxide-network     Device REST API (axum), OTA update engine
├── oxide-control     Device registry, fleet manager, model store, control plane
└── oxide-cli         The binary you actually run (11 subcommands)
```

Devices that only need inference can depend on `oxide-runtime` + `oxide-models` alone. The control plane, networking, and security layers are opt-in.

### OTA update protocol

```
1. STAGE     Write model to staging/
2. VERIFY    SHA-256 against manifest
3. BACKUP    Current model → backup/
4. APPLY     staging/ → active/  (atomic)
5. HEALTH    Load model, run test inference
6. CONFIRM   Clean staging — or ROLLBACK backup/ → active/
```

### Agent heartbeat loop

```
Agent                          Control Plane
  │                                │
  ├── POST /heartbeat ────────────→│  (device state, metrics)
  │                                │
  │←───────── 200 OK ─────────────┤  (assigned model + version, or null)
  │                                │
  ├── GET /models/.../download ───→│  (if new assignment)
  │←───────── model bytes ────────┤
  │                                │
  ├── [stage → verify → apply] ──→│
  ├── [health check: load + infer]│
  │                                │
  ├── POST /heartbeat ────────────→│  (report success / failure)
  │                                │
  └── sleep(poll_interval) ───────→  repeat
```

### Inference pipeline

```
ONNX file → tract-onnx (parse + optimize) → tract-core (SIMD: NEON/AMX/AVX) → output
```

tract auto-detects your hardware. On this M4 Pro it enables ARMv8.2 half-precision, Apple AMX matrix extensions, and fused sigmoid/tanh — automatically, without flags.

---

## Testing

121 tests. Three tiers.

```bash
# Everything
cargo test --workspace               # 82 unit + 28 integration + 11 stress

# Just stress tests (concurrent inference, 100-device fleet, 20 OTA versions)
cargo test -p oxide-cli --test stress_tests

# Full E2E: builds binary, starts server, runs curl against API, checks outputs
bash tests/run_all.sh
```

The stress tests include:
- 10,000 sequential inferences with metric verification
- 4 threads × 1,000 concurrent inferences on a shared engine
- 50 model load/unload cycles (hot-swap simulation)
- 100-device fleet deployment
- 20 sequential OTA version upgrades with rollback chain
- Encryption across payload sizes from 0 to 64 KB

---

## Configuration

```toml
# oxide.toml
[runtime]
model_dir = "./models"
max_memory_bytes = 52428800   # 50 MB budget
num_threads = 0               # 0 = auto-detect
enable_simd = true

[security]
encrypt_models = false
# key_file = "./oxide.key"

[network]
listen_addr = "0.0.0.0"
listen_port = 8090
heartbeat_interval_secs = 30

[telemetry]
enabled = true
report_interval_secs = 60
max_queue_size = 1000         # Offline telemetry buffer
```

---

## Platform support

| Platform | Arch | Status |
|----------|------|:------:|
| macOS | Apple Silicon (aarch64) | ✅ Tested |
| Linux | aarch64 (Pi, Jetson) | ✅ Compiles |
| Linux | x86_64 | ✅ Compiles |
| Windows | x86_64 | ✅ Compiles |

Cross-compile for Raspberry Pi:
```bash
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
```

---

## Roadmap

- [x] ONNX inference with SIMD acceleration
- [x] Model encryption (AES-256-GCM)
- [x] OTA deploys with atomic rollback
- [x] Fleet management with canary/rolling rollouts
- [x] Control plane HTTP API with model store
- [x] Agent daemon with pull-based OTA updates
- [x] Benchmarking CLI
- [ ] mTLS device ↔ control plane
- [ ] Prometheus metrics endpoint
- [ ] TensorFlow Lite support
- [ ] Python SDK for model prep + deployment scripting
- [ ] Kubernetes operator

---

## Development

```bash
cargo build --workspace                       # Debug build
cargo build --release -p oxide-cli            # Release (6 MB)
cargo test --workspace                        # All 121 tests
cargo clippy --workspace                      # Zero warnings (all + nursery)
bash tests/run_all.sh                         # Full E2E
python3 models/generate_test_models.py        # Regenerate test models
```

---

## License

MIT or Apache 2.0, at your option.
