# Oxide v2: ML-Aware OTA Platform

## Problem

Deploying ML model updates to device fleets is expensive and fragile.

A 500 MB model pushed to 1,000 devices = 500 GB of bandwidth — per update.
But fine-tuning typically changes <20% of weights. 80%+ of that transfer is
redundant bytes the device already has.

Existing OTA tools (Mender, RAUC, SWUpdate, Balena) treat model files as
opaque blobs. They can do binary diffs, but they don't understand what's
inside. They diff at the byte level, missing the structure of ML models.

Oxide v2 focuses on **ML-aware delta updates**: understanding that an ONNX
file is 99.9% float32 weight tensors, and that fine-tuning changes a small
subset of those tensors by small amounts.

## Evidence

All numbers measured against MLP-MNIST (2 MB, 535K params, 6 tensors).

### Naive merkle trees don't work for ML models

Block-level hashing (the merkle approach) fails when changes are sparse
but spread across all blocks — which is exactly what happens during
fine-tuning:

```
Scenario: All layers, 5% of weights perturbed

  Actual bytes changed:     87,721 / 2,143,752 (4.1%)
  Naive merkle (4KB blocks):
    Dirty blocks: 524 / 524 (100%)  ← EVERY block is dirty
    Must ship:    2,143,752 bytes   ← zero savings
```

Even though only 4.1% of bytes changed, because the changes are scattered
across all tensors, every 4KB block contains at least one changed float.
A merkle tree at the block level sees everything as dirty.

### What works: XOR delta + compression

The changed bytes are sparse within each block. XOR delta (base ⊕ target)
produces a mostly-zero byte stream that compresses extremely well:

```
Approach comparison (all layers, 5% change):

  Full file:                     2,143,752 bytes  (100.0%)
  Naive merkle 4KB:              2,143,752 bytes  (100.0%)  ← useless
  Merkle + XOR compressed:         150,350 bytes  (  7.0%)
  zstd --patch-from:               116,643 bytes  (  5.4%)
  Tensor XOR compressed:           150,342 bytes  (  7.0%)
```

### Tensor-level hashing wins for structured changes

When changes are localized to specific layers (transfer learning, layer
freezing), tensor-level hashing dominates — skip entire unchanged tensors:

```
Scenario: Transfer learning (last layer only)

  Tensor inventory:
    w1 (1,605,632 bytes)  ca1fa7e41969 → ca1fa7e41969  [SAME]
    b1 (    2,048 bytes)  e5a00aa9991a → e5a00aa9991a  [SAME]
    w2 (  524,288 bytes)  65c87efb4860 → 65c87efb4860  [SAME]
    b2 (    1,024 bytes)  5f70bf18a086 → 5f70bf18a086  [SAME]
    w3 (   10,240 bytes)  1e8ef38f558c → 0e7432db6cc7  [DIFF]
    b3 (       40 bytes)  2c34ce1df23b → 113128c3f695  [DIFF]

  Full file:         2,143,752 bytes  (100.0%)
  Tensor XOR delta:      9,224 bytes  (  0.4%)  ← 99.6% savings
```

### Neither approach wins universally

```
Scenario                            Full    zstd Δ  Tensor Δ  Winner
──────────────────────────────  ────────  ────────  ────────  ──────
Transfer learning (last layer)    2094K       10K       9K   tensor
Last 2 layers retrained           2094K      484K     468K   tensor
All layers, 5% perturb            2094K      121K     154K   zstd
All layers, 20% perturb           2094K      443K     535K   zstd
All layers, 50% perturb           2094K     1040K    1175K   zstd
Full retrain                      2094K     1942K    1981K   zstd
```

**Tensor-level wins** for structured changes (transfer learning, layer
freezing). **Binary delta wins** for diffuse changes (full fine-tuning,
distillation). The right strategy: compute both, ship whichever is smaller.

### Worst case: full retrain

When the model is completely retrained (100% of weights change), delta
compression still saves ~7% via zstd's dictionary matching. But the real
answer is: **that's fine**. Full retrains are rare. The common case — daily
fine-tuning, A/B weight experiments, quantization adjustments — is where
90%+ savings happen.

The protocol gracefully degrades: if the delta is larger than the full
file, just ship the full file. The agent doesn't care.

## Why This Matters: Real-World Numbers

### The bandwidth problem is severe on cellular IoT

Most edge ML fleets connect over LTE Cat-M1 or NB-IoT:

| Network    | Real-world throughput | 100 MB download time | Cost per GB      |
|------------|----------------------|----------------------|------------------|
| Cat-M1     | ~300 kbps            | ~45 minutes          | $5–30/GB         |
| NB-IoT     | ~26–127 kbps         | 2–8 hours            | $5–50/GB         |
| WiFi/Eth   | 10+ Mbps             | seconds              | ~free            |

At $5–30/GB (Hologram, 1NCE, carrier IoT plans), pushing a 100 MB model
to 1,000 devices costs **$500–$3,000 per update** in cellular data alone.
With delta updates at 10% of file size, that drops to $50–$300.

For a company doing weekly model updates, that's $25K–$150K/year in
cellular data savings — from one feature.

### Edge vision models are not small

| Model              | ONNX size (fp32) | Typical use case            |
|--------------------|-----------------:|-----------------------------| 
| YOLOv8-nano        |         ~12 MB   | Real-time object detection  |
| MobileNetV3-Large  |         ~22 MB   | Image classification        |
| EfficientNet-Lite0 |         ~20 MB   | Efficient classification    |
| YOLOv8-small       |         ~45 MB   | Higher-accuracy detection   |
| ResNet-50          |        ~100 MB   | Feature extraction          |
| BERT-base          |        ~440 MB   | NLP on edge                 |

These are the small, edge-optimized models. Production models with custom
heads, ensembles, or domain-specific architectures are often 200–500 MB.

### No one is solving this

Research findings (March 2026):

**Existing OTA tools don't understand ML models:**
- Mender, RAUC, SWUpdate — filesystem/partition-level updates, treat
  models as opaque blobs. Mender has binary delta (mender-binary-delta)
  but it's an add-on, not ML-aware.
- Balena — container-based, proprietary delta for Docker layers. Not
  applicable to model files inside containers.

**Cloud ML platforms stop at "push container":**
- Azure IoT Edge, AWS Greengrass, NVIDIA Fleet Command — deploy Docker
  containers. No model-level delta. No bandwidth optimization.
- <70% of Edge AI projects stall in pilot phase partly due to deployment
  operational hurdles.

**Edge ML platforms don't do OTA:**
- Edge Impulse — model training and optimization, some fleet management,
  but no delta updates.
- TFLite, ONNX Runtime — inference only, zero deployment tooling.

**No one does tensor-level diffing:**
- Google Chrome's Courgette achieves 89% smaller patches than bsdiff by
  understanding executable structure — but it's specific to compiled code
  (disassembly + address normalization). Not applicable to weight tensors.
- There are Rust crates for binary delta (bsdiff-rs, bidiff) but none that
  understand ML model internals.
- SafeTensors (Hugging Face) has a simpler structure than ONNX (JSON header
  + flat tensor data, no protobuf) — even easier to diff at tensor level.
  Supporting it alongside ONNX is straightforward.

**The gap is real.** Companies doing edge ML are either:
1. Shipping full model files every time (expensive, slow)
2. Building custom deployment scripts (fragile, unmaintained)
3. Using cloud platform containers (heavy, no model awareness)

None of these handle the "I fine-tuned the last layer, deploy to 1,000
cameras" case efficiently.

## Positioning

**"Mender for ML models."**

Not a general OS updater. Not an inference runtime. A purpose-built tool for
getting updated ML models onto device fleets efficiently and safely.

Oxide keeps:
- OTA lifecycle (stage → verify → backup → apply → health check → rollback)
- Agent heartbeat loop (pull-based, works through NATs)
- Fleet management (register, group, deploy, monitor)
- Model encryption (AES-256-GCM for IP protection)

Oxide drops:
- The inference engine wrapper (let users bring their own runtime)
- Pretending to compete with tract/onnxruntime/TFLite on inference

Oxide adds:
- Delta updates (tensor-level AND binary-level, pick smaller)
- Model format awareness (ONNX protobuf structure)
- Bandwidth monitoring and reporting
- Update campaigns with progress tracking

## Architecture

### Current (v1)

```
Control Plane                          Device Agent
┌──────────────┐                    ┌──────────────────┐
│ Model Store  │  full file         │ OTA Updater      │
│ (filesystem) │ ──────────────────→│ (stage/verify/   │
│              │  N bytes           │  apply/rollback) │
│ Registry     │                    │                  │
│ Fleet Mgr    │                    │ Inference Engine │
└──────────────┘                    └──────────────────┘
```

### Proposed (v2)

```
Control Plane                              Device Agent
┌──────────────────┐                    ┌──────────────────┐
│ Model Store      │                    │                  │
│   + version DAG  │  delta patch       │ OTA Updater      │
│   + delta cache  │ ──────────────────→│  + delta apply   │
│                  │  ~10% of N bytes   │  + reconstruction│
│ Delta Engine     │                    │  + verify full   │
│   (tensor diff   │                    │                  │
│    + binary diff  │  ←── heartbeat ── │ Model Inventory  │
│    pick smaller) │  (tensor hashes)   │ (what's on disk) │
│                  │                    │                  │
│ Registry         │                    │ [user's runtime] │
│ Fleet Mgr        │                    │                  │
│ Campaign Tracker │                    │                  │
└──────────────────┘                    └──────────────────┘
```

### Key changes

1. **Drop `oxide-models` and `oxide-runtime` inference wrappers.**
   The agent no longer loads or runs models. It delivers files. Health
   checks become user-supplied hooks (a script or binary that the agent
   calls post-update).

2. **Add `oxide-delta` crate.** Two delta strategies, best wins:

   **Strategy A: Tensor-level delta** (ML-aware)
   - Parse ONNX protobuf to extract tensor boundaries
   - Hash each tensor (SHA-256) — this is the "merkle" layer
   - For matching tensors: emit COPY chunk (zero bytes)
   - For different tensors: emit XOR delta, zstd-compressed
   - Best for: transfer learning, layer freezing, targeted changes

   **Strategy B: Binary delta** (format-agnostic)
   - zstd `--patch-from` style dictionary compression
   - No format knowledge needed — works on any file
   - Best for: diffuse fine-tuning where all weights shift slightly

   Control plane computes both, caches the smaller one.

3. **Agent sends tensor manifest in heartbeat.** The agent reports
   per-tensor SHA-256 hashes (192 bytes for a 6-tensor model). The
   control plane uses this to determine the minimal delta.

4. **Campaign tracker.** A deployment is a "campaign" with per-device
   progress tracking, pause/resume/abort.

## Delta Strategies in Detail

### Strategy A: Tensor-Level Delta

The "merkle tree" — but at the tensor level, not the block level.

Why not block-level merkle? Because ML weight changes are sparse within
blocks. A 4KB block containing 1024 floats where 50 changed (5%) still
hashes differently. You'd ship the whole block. With tensor-level
granularity, you skip entire unchanged tensors (often the largest ones).

```
Agent manifest:              Target manifest:
  w1: ca1fa7e4...              w1: ca1fa7e4...  ← SAME, skip
  b1: e5a00aa9...              b1: e5a00aa9...  ← SAME, skip
  w2: 65c87efb...              w2: 65c87efb...  ← SAME, skip
  b2: 5f70bf18...              b2: 5f70bf18...  ← SAME, skip
  w3: 1e8ef38f...              w3: 0e7432db...  ← DIFF, ship delta
  b3: 2c34ce1d...              b3: 113128c3...  ← DIFF, ship delta

Delta payload: XOR(w3_old, w3_new) + XOR(b3_old, b3_new), zstd compressed
Result: 9,224 bytes instead of 2,143,752 (99.6% savings)
```

For changed tensors, the XOR delta is sparse (mostly zeros where weights
didn't change) and compresses extremely well.

### Strategy B: Binary Delta

Standard dictionary-based delta compression. The control plane uses the
base version as a zstd dictionary to compress the target version. No
format knowledge needed.

This wins when changes are spread uniformly across all tensors (common
with learning rate warm-up, batch normalization updates, or full
fine-tuning passes) because there are no "clean" tensors to skip.

### Decision logic (on the control plane)

```
on model upload(model_id, version, bytes):
    for each previous version:
        delta_a = tensor_level_delta(prev_bytes, bytes)  // may fail if not ONNX
        delta_b = binary_delta(prev_bytes, bytes)
        
        best = min(delta_a, delta_b, bytes)  // pick smallest, including full file
        cache(model_id, prev_version → version, best)
```

## Download Protocol

### Request

```
GET /api/v1/models/{id}/versions/{ver}/download
X-Oxide-Base-Version: v1.0.0
X-Oxide-Base-SHA256: abc123...
X-Oxide-Tensor-Manifest: w1=ca1fa7e4...,b1=e5a00aa9...,...
```

The tensor manifest is optional. Without it, the server can still serve
binary deltas (strategy B). With it, the server can compute tensor-level
deltas on the fly if not cached.

### Response (delta available)

```
200 OK
Content-Type: application/x-oxide-delta
X-Oxide-Delta-Strategy: tensor | binary
X-Oxide-Delta-Base: v1.0.0
X-Oxide-Target-SHA256: def456...
X-Oxide-Target-Size: 2143752

body = delta patch bytes
```

### Response (no delta, or full file is smaller)

```
200 OK
Content-Type: application/octet-stream
X-Oxide-SHA256: def456...

body = full model bytes
```

Fully backward compatible. Old agents without the headers get full files.

## Patch Format (OXDL v1)

```
┌─────────────────────────────────────────┐
│ Header (80 bytes)                       │
│   magic: "OXDL" (4 bytes)              │
│   version: u8                           │
│   strategy: u8 (0=binary, 1=tensor)    │
│   base_sha256: [u8; 32]                │
│   target_sha256: [u8; 32]              │
│   target_size: u64                      │
│   num_chunks: u32                       │
│   compression: u8 (0=none, 1=zstd)     │
│   reserved: [u8; 3]                     │
├─────────────────────────────────────────┤
│ Chunk 0                                 │
│   name_len: u16                         │
│   name: [u8; name_len]  (tensor name)  │
│   offset: u64  (in target file)        │
│   length: u32  (uncompressed)          │
│   op: u8 (0=COPY, 1=REPLACE, 2=XOR)   │
│   data_len: u32 (compressed, 0 for COPY)│
│   data: [u8; data_len]                 │
├─────────────────────────────────────────┤
│ Chunk 1...                              │
└─────────────────────────────────────────┘
```

**Chunk operations:**
- `COPY`: This region is identical in base and target. Zero data bytes.
  Device copies from base file at the given offset.
- `REPLACE`: This region is entirely new. Data contains the raw
  (compressed) target bytes. Used for new tensors or non-tensor regions.
- `XOR`: This region differs. Data contains `base[offset..] ⊕ target[offset..]`,
  compressed. Device XORs against its base to reconstruct target.

For tensor-level patches, each chunk corresponds to one tensor.
For binary patches, chunks are arbitrary byte ranges.

## Application on Device

```
fn apply_delta(base_path: &Path, patch: &[u8]) -> Result<Vec<u8>>:
    1. Parse header, verify base_sha256 matches local file
    2. Read base file into memory
    3. Allocate target buffer (target_size from header)
    4. For each chunk:
       COPY:    memcpy from base at offset for length bytes
       REPLACE: decompress chunk data, write at offset
       XOR:     decompress chunk data, XOR with base at offset, write
    5. Verify target_sha256 of reconstructed file
    6. Write to staging (existing OTA pipeline takes over)
```

Step 5 is critical: if reconstruction produces the wrong SHA-256,
something went wrong. Discard and fall back to full download.

## Campaign Tracking

A campaign replaces the current fire-and-forget deploy.

```rust
struct Campaign {
    id: CampaignId,
    model_id: ModelId,
    target_version: ModelVersion,
    fleet_id: FleetId,
    strategy: RolloutStrategy,
    state: CampaignState,  // pending / rolling_out / paused / complete / aborted
    created_at: DateTime<Utc>,

    // Per-device tracking
    devices: HashMap<DeviceId, DeviceUpdateState>,

    // Bandwidth stats
    total_bytes_served: u64,
    total_bytes_saved_by_delta: u64,
}

enum DeviceUpdateState {
    Pending,
    Downloading { started_at: DateTime<Utc>, bytes_total: u64, delta_strategy: String },
    Applying,
    Verifying,
    Complete { completed_at: DateTime<Utc>, bytes_downloaded: u64, delta_ratio: f64 },
    Failed { error: String, attempts: u32 },
    Skipped { reason: String },  // already on target version
}
```

API additions:
```
POST  /api/v1/campaigns                    Create campaign (replaces deploy)
GET   /api/v1/campaigns/:id                Campaign status + per-device state
POST  /api/v1/campaigns/:id/pause          Pause rollout
POST  /api/v1/campaigns/:id/resume         Resume rollout
POST  /api/v1/campaigns/:id/abort          Abort (leave devices as-is)
GET   /api/v1/campaigns/:id/devices        Detailed per-device breakdown
GET   /api/v1/campaigns/:id/bandwidth      Bandwidth savings report
```

## Health Check Hooks

Instead of Oxide running inference, the user supplies a health check:

```toml
# oxide-agent.toml
[health_check]
command = "/opt/myapp/check_model.sh"
timeout_seconds = 30
```

The agent calls the command after applying an update. Exit code 0 = healthy,
non-zero = rollback. Stdout/stderr captured and reported to the control plane.

This lets users:
- Run inference with their own runtime
- Check model output quality against known inputs
- Verify the model integrates with their application
- Do hardware-specific validation (GPU load, memory usage)

## Implementation Plan

### Phase 1: oxide-delta crate

Rust dependencies:
- `prost` — ONNX protobuf parsing (generate from onnx.proto3)
- `zstd` — compression for binary delta strategy
- `sha2` — already in workspace, SHA-256 for manifests and verification
- `bsdiff-rs` or hand-rolled XOR — for tensor-level chunk deltas
  (XOR is simpler and sufficient since we control both sides)

Tasks:
- [ ] ONNX protobuf parser: extract tensor names, offsets, sizes, raw data
- [ ] SafeTensors parser: JSON header + flat tensor extraction (simpler)
- [ ] Tensor manifest: compute per-tensor SHA-256 hashes
- [ ] Strategy A: tensor-level XOR delta (COPY/XOR chunks per tensor)
- [ ] Strategy B: binary delta (zstd dictionary compression)
- [ ] Patch format: write and read OXDL v1
- [ ] `compute_delta(base, target) -> Patch` — tries both, picks smaller
- [ ] `apply_delta(base, patch) -> target` — reconstruct + verify SHA-256
- [ ] Tests: round-trip every scenario with real ONNX models
- [ ] Benchmark: measure delta computation time (must be <5s for 500 MB model)

### Phase 2: Control plane integration
- [ ] Delta cache: compute and store deltas on model upload
- [ ] Download endpoint: serve delta or full based on agent headers
- [ ] Campaign model: replace fire-and-forget deploy
- [ ] Campaign API: create / status / pause / resume / abort
- [ ] Bandwidth tracking: log bytes served per device per campaign

### Phase 3: Agent updates
- [ ] Delta-aware download: send base version + tensor manifest in headers
- [ ] Delta application: reconstruct target from base + patch
- [ ] Fallback: if delta reconstruction fails SHA-256 check, download full
- [ ] Health check hooks: call user-supplied command instead of inference
- [ ] Campaign reporting: report download/apply/verify progress per heartbeat

### Phase 4: Drop inference wrapper
- [ ] Remove oxide-models dependency from oxide-cli agent
- [ ] Remove oxide-runtime dependency from oxide-cli agent
- [ ] Agent becomes a pure delivery + lifecycle tool
- [ ] Update README, demo, docs

### Phase 5: Production hardening (separate effort)
- [ ] SQLite persistence (replace JSON files)
- [ ] mTLS or JWT auth
- [ ] Rate limiting on control plane
- [ ] Horizontal scaling (stateless control plane + shared DB)
- [ ] Prometheus metrics endpoint

## Success Metrics

1. **Transfer learning delta**: <1% of full model size
2. **Fine-tuning delta**: <15% of full model size
3. **Round-trip correctness**: SHA-256 of reconstructed model matches
   original — always, no exceptions
4. **Graceful degradation**: full retrain → ship full file, no worse than today
5. **Backward compat**: agents without delta support still work
6. **Campaign visibility**: know which devices have which version at any time

## Open Questions

1. **SafeTensors support (high priority).** SafeTensors (Hugging Face) has
   an even simpler structure than ONNX: 8-byte header length + JSON
   metadata + flat contiguous tensor data. No protobuf, no graph structure.
   Tensor boundaries are explicit in the JSON header (offset + length).
   This makes it trivially diffable. Supporting both ONNX + SafeTensors
   covers the two dominant edge model formats. Binary fallback covers
   everything else (TFLite, PyTorch .pt, custom formats).

2. **Should deltas be computed eagerly or lazily?** Eager (on upload) is
   simpler and avoids latency on first download. Lazy saves storage if
   most version pairs are never requested. Start eager, add lazy eviction
   later.

3. **Multi-hop deltas?** If a device is on v1 and target is v5, should we
   ship v1→v5 directly, or chain v1→v2→v3→v4→v5? Direct is simpler and
   smaller. Chain reuses cached deltas. Start with direct.

4. **Quantized diffs (Level 2)?** If a weight changed from 0.50000 to
   0.50012, do we need the exact delta or is "approximately +0.0001"
   good enough? This is lossy and model-quality-sensitive. Defer to a
   future version with user-configurable tolerance.

5. **Should the tensor manifest live in the heartbeat or a separate
   endpoint?** Heartbeat keeps it simple (one round-trip). But for models
   with thousands of tensors (large transformers: 200+ tensors), the
   manifest could be several KB. Probably fine in the heartbeat body for
   now, consider a separate `POST /manifest` later.
