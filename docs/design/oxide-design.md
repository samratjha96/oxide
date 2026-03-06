# Oxide Design

## What is Oxide?

Oxide delivers ML model updates to device fleets using the minimum
possible bandwidth. It understands model file structure — tensors,
weights, layers — and ships only what changed.

One binary. Control plane + device agent. ONNX and SafeTensors aware.
Falls back to binary delta for any other format. Pull-based (works
through NATs and firewalls). Rollback on failure. Encrypted at rest.

## The Problem

Deploying ML model updates to device fleets is expensive and fragile.

A 500 MB model pushed to 1,000 devices = 500 GB of bandwidth per update.
But fine-tuning typically changes <20% of weights. 80%+ of that transfer
is redundant bytes the device already has.

### The bandwidth problem is real

Most edge ML fleets connect over cellular:

| Network    | Real-world throughput | 100 MB download | Cost per GB |
|------------|----------------------|------------------|-------------|
| Cat-M1     | ~300 kbps            | ~45 minutes      | $5–30       |
| NB-IoT     | ~26–127 kbps         | 2–8 hours        | $5–50       |
| WiFi/Eth   | 10+ Mbps             | seconds          | ~free       |

1,000 devices × 100 MB model × weekly update = **$25K–$150K/year** in
cellular data alone. With delta updates at 10%, that drops to $2.5K–$15K.

### Edge models are not small

| Model              | ONNX size (fp32) | Use case                    |
|--------------------|-----------------:|-----------------------------|
| YOLOv8-nano        |         ~12 MB   | Real-time object detection  |
| MobileNetV3-Large  |         ~22 MB   | Image classification        |
| EfficientNet-Lite0 |         ~20 MB   | Efficient classification    |
| YOLOv8-small       |         ~45 MB   | Higher-accuracy detection   |
| ResNet-50          |        ~100 MB   | Feature extraction          |
| BERT-base          |        ~440 MB   | NLP on edge                 |

Production models with custom heads or ensembles: 200–500 MB.

### Nobody is solving this

**OTA tools** (Mender, RAUC, SWUpdate) — filesystem/partition-level.
Models are opaque blobs. Mender has binary delta as an add-on but it's
not ML-aware.

**Cloud platforms** (Azure IoT Edge, Greengrass, Fleet Command) — push
Docker containers. No model-level delta. No bandwidth optimization.

**Edge ML platforms** (Edge Impulse, TFLite) — training and inference.
No fleet OTA. No delta updates.

**Binary diff tools** (bsdiff, Courgette) — Chrome's Courgette gets 89%
smaller patches than bsdiff by understanding executable structure. But it
disassembles code — not applicable to weight tensors.

No existing tool does tensor-level diffing of ML models.

## Evidence

All numbers measured against MLP-MNIST (2 MB, 535K params, 6 tensors).

### Block-level hashing doesn't work for ML

When 5% of weights change across all layers, every 4KB block is dirty:

```
Actual bytes changed:     87,721 / 2,143,752 (4.1%)
Naive merkle (4KB blocks):
  Dirty blocks: 524 / 524 (100%)  ← EVERY block is dirty
  Must ship:    2,143,752 bytes   ← zero savings
```

Changes are sparse within blocks. A block with 1024 floats where 50
changed still hashes differently.

### XOR delta + compression works

XOR delta (base ⊕ target) produces a mostly-zero stream that compresses
well:

```
Full file:                     2,143,752 bytes  (100.0%)
Naive merkle 4KB:              2,143,752 bytes  (100.0%)  ← useless
zstd --patch-from:               116,643 bytes  (  5.4%)
Tensor XOR compressed:           150,342 bytes  (  7.0%)
```

### Tensor-level hashing wins for structured changes

Transfer learning (last layer only):

```
w1 (1,605,632 bytes)  [SAME] — skip
b1 (    2,048 bytes)  [SAME] — skip
w2 (  524,288 bytes)  [SAME] — skip
b2 (    1,024 bytes)  [SAME] — skip
w3 (   10,240 bytes)  [DIFF] — ship delta
b3 (       40 bytes)  [DIFF] — ship delta

Full file:       2,143,752 bytes  (100.0%)
Tensor delta:        9,224 bytes  (  0.4%)  ← 99.6% savings
```

### No single strategy wins everywhere

```
Scenario                          Full   zstd Δ  Tensor Δ  Winner
────────────────────────────  ───────  ───────  ────────  ──────
Transfer learning (last layer)  2094K     10K       9K    tensor
Last 2 layers retrained         2094K    484K     468K    tensor
All layers, 5% perturb          2094K    121K     154K    zstd
All layers, 50% perturb         2094K   1040K    1175K    zstd
Full retrain                    2094K   1942K    1981K    zstd
```

**Oxide computes both, ships whichever is smaller.** Falls back to full
file if delta is larger. The device doesn't care which strategy was used.

## Architecture

```
Control Plane                              Device Agent
┌──────────────────┐                    ┌──────────────────┐
│ Model Store      │                    │                  │
│   + delta cache  │  delta patch       │ OTA Pipeline     │
│                  │ ──────────────────→│  stage           │
│ Delta Engine     │  ~10% of N bytes   │  reconstruct     │
│   tensor diff    │                    │  verify sha256   │
│   binary diff    │  ←── heartbeat ──  │  backup          │
│   pick smaller   │  (tensor hashes)   │  apply           │
│                  │                    │  health check    │
│ Registry         │                    │  [rollback]      │
│ Fleet Manager    │                    │                  │
│ Campaign Tracker │                    │ [user's runtime] │
└──────────────────┘                    └──────────────────┘
```

The agent does not run inference. It delivers model files and manages the
update lifecycle. Health checks are user-supplied hooks — any script or
binary that returns exit 0 for healthy.

### Delta engine (oxide-delta crate)

Two strategies. Best wins.

**Strategy A — Tensor-level delta** (ML-aware):
- Parse ONNX/SafeTensors to find tensor boundaries
- SHA-256 each tensor — this is the "merkle" layer
- Matching tensors → COPY chunk (zero bytes)
- Different tensors → XOR delta, zstd-compressed
- Best for: transfer learning, layer freezing, targeted changes

**Strategy B — Binary delta** (format-agnostic):
- zstd dictionary compression (base as dictionary)
- No format knowledge needed — works on any file
- Best for: diffuse fine-tuning, all weights shift slightly

```
on model upload(model_id, version, bytes):
    for each previous version:
        delta_a = tensor_level_delta(prev_bytes, bytes)
        delta_b = binary_delta(prev_bytes, bytes)
        best = smallest of (delta_a, delta_b, bytes)
        cache(model_id, prev → version, best)
```

### Download protocol

Agent sends what it has. Server sends the smallest possible response.

```
GET /api/v1/models/{id}/versions/{ver}/download
X-Oxide-Base-Version: v1.0.0
X-Oxide-Base-SHA256: abc123...
X-Oxide-Tensor-Manifest: w1=ca1fa7e4...,b1=e5a00aa9...
```

Server responds with delta (if smaller) or full file. Content-Type
distinguishes: `application/x-oxide-delta` vs `application/octet-stream`.

Old agents without the headers get full files. Fully backward compatible.

### Patch format (OXDL)

```
┌─────────────────────────────────┐
│ Header (80 bytes)               │
│   magic: "OXDL"                 │
│   strategy: tensor | binary     │
│   base_sha256, target_sha256    │
│   target_size, num_chunks       │
│   compression: zstd             │
├─────────────────────────────────┤
│ Chunk 0                         │
│   name (tensor name or "")      │
│   offset, length                │
│   op: COPY | REPLACE | XOR      │
│   data (compressed, 0 for COPY) │
├─────────────────────────────────┤
│ Chunk 1...                      │
└─────────────────────────────────┘
```

COPY = identical region, zero bytes shipped.
REPLACE = new data, raw compressed bytes.
XOR = base ⊕ target at offset, compressed.

Reconstruction on device:

```
1. Verify base_sha256 matches local file
2. For each chunk: COPY from base, REPLACE with data, or XOR with base
3. Verify target_sha256 of result
4. If mismatch → discard, fall back to full download
5. Write to staging → existing OTA pipeline takes over
```

### Campaigns

A deployment is a campaign with per-device progress tracking.

```rust
struct Campaign {
    id: CampaignId,
    model_id: ModelId,
    target_version: ModelVersion,
    fleet_id: FleetId,
    strategy: RolloutStrategy,
    state: CampaignState,

    devices: HashMap<DeviceId, DeviceUpdateState>,
    total_bytes_served: u64,
    total_bytes_saved_by_delta: u64,
}
```

States: Pending → Downloading → Applying → Verifying → Complete | Failed.
Campaigns can be paused, resumed, or aborted. Bandwidth savings tracked
per device and per campaign.

### Health check hooks

```toml
# oxide-agent.toml
[health_check]
command = "/opt/myapp/check_model.sh"
timeout_seconds = 30
```

Exit 0 = healthy. Non-zero = rollback. Stdout/stderr reported to the
control plane. Users run whatever validation they want — inference with
their own runtime, output quality checks, hardware validation.

## Implementation Plan

### Phase 1: oxide-delta

The core differentiator. Pure Rust crate, no network, no server.

Dependencies: `prost` (ONNX protobuf), `zstd`, `sha2`.

- [x] ONNX parser: extract tensor names, offsets, sizes, raw bytes
- [x] SafeTensors parser: JSON header + flat tensor extraction
- [x] Tensor manifest: per-tensor SHA-256 hashes
- [x] Strategy A: tensor-level XOR delta
- [x] Strategy B: binary delta (zstd dictionary)
- [x] OXDL patch format: write and read
- [x] `compute_delta(base, target) -> Patch` — both strategies, pick smaller
- [x] `apply_delta(base, patch) -> target` — reconstruct + SHA-256 verify
- [x] Round-trip tests with real ONNX models (9 tests)
- [ ] Benchmark: <5s for 500 MB model delta computation

### Phase 2: Control plane

- [x] Delta cache on model upload
- [x] Download endpoint: delta or full based on headers
- [x] Campaign model (replace fire-and-forget deploy)
- [x] Campaign API: create / status / pause / resume / abort / bandwidth
- [x] Bandwidth tracking per device per campaign

### Phase 3: Agent

- [x] Send base version in download request
- [x] Delta download and reconstruction
- [x] SHA-256 fallback: if reconstruction fails, download full
- [ ] Health check hooks (replace built-in inference)
- [ ] Campaign progress reporting per heartbeat

### Phase 4: Cleanup

- [ ] Remove inference engine from agent
- [ ] Agent = delivery + lifecycle only
- [ ] Update README, demo, docs

### Phase 5: Harden (separate)

- [ ] SQLite (replace JSON files)
- [ ] mTLS or JWT
- [ ] Rate limiting
- [ ] Prometheus metrics

## Success Metrics

1. Transfer learning delta: **<1% of full model**
2. Fine-tuning delta: **<15% of full model**
3. Round-trip: **SHA-256 match, always**
4. Graceful degradation: full retrain → full file, no worse than before
5. Backward compatible: old agents still work
6. Campaign visibility: exact per-device version state at all times

## Open Questions

1. **SafeTensors first?** Simpler than ONNX (JSON header, flat tensors,
   no protobuf). Could be the faster path to prove the delta engine. ONNX
   needs protobuf generation. SafeTensors needs a JSON parser and byte
   offset math.

2. **Eager vs lazy delta computation?** Eager on upload is simpler. Lazy
   saves storage. Start eager.

3. **Multi-hop deltas?** Device on v1, target v5. Ship v1→v5 direct or
   chain v1→v2→...→v5? Direct is simpler. Start there.

4. **Tensor manifest in heartbeat or separate endpoint?** For models with
   200+ tensors the manifest is several KB. Fine in heartbeat body for
   now.

5. **Quantized diffs?** Weight changed by 0.001 — ship exact delta or
   approximate? Lossy, model-quality-sensitive. Defer.
