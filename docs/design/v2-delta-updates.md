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

Simulated fine-tuning of MLP-MNIST (2 MB, 535K params), 10% of weights
perturbed:

```
Original model (v1):           2,143,752 bytes
Updated model (v2):            2,143,752 bytes
v2 compressed (standalone):    1,977,128 bytes  (92.2%)
v2 delta (zstd --patch-from):    221,536 bytes  (10.3%)

Bandwidth saved with delta:   89.7%

At 1000 devices:
  Full push:  2,044 MB
  Delta push:    211 MB
```

Even generic binary delta (zstd patch-from) gets 90% savings. ML-aware
delta — diffing at the tensor level — can do better by:
- Skipping unchanged tensors entirely (zero bytes for unchanged layers)
- Quantizing weight diffs (if a weight changed by 0.001, we don't need
  full float32 precision for the delta)
- Compressing per-tensor rather than per-file (better compression ratios
  when data within a tensor is homogeneous)

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
- Delta updates (tensor-level and binary-level)
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
│   (tensor diff)  │                    │                  │
│                  │  ←── heartbeat ──  │ Model Inventory  │
│ Registry         │  (current version) │ (what's on disk) │
│ Fleet Mgr        │                    │                  │
│ Campaign Tracker │                    │ [user's runtime] │
└──────────────────┘                    └──────────────────┘
```

### Key changes

1. **Drop `oxide-models` and `oxide-runtime` inference wrappers.**
   The agent no longer loads or runs models. It delivers files. Health
   checks become user-supplied hooks (a script or binary that the agent
   calls post-update).

2. **Add `oxide-delta` crate.** Computes and applies model diffs:
   - Level 0: zstd `--patch-from` (binary delta, no ML knowledge)
   - Level 1: ONNX-aware — diff individual tensors, skip unchanged ones
   - Level 2: Quantized tensor diffs (future — lossy compression of
     weight deltas within user-specified tolerance)

3. **Control plane computes deltas on upload.** When you upload v2, the
   server diffs it against v1 and caches the patch. Devices request
   deltas, not full files.

4. **Agent tracks local model inventory.** Instead of downloading
   whatever is assigned, the agent reports what it has and the control
   plane tells it the minimal path to the target version.

5. **Campaign tracker.** A deployment is now a "campaign" — tracks
   per-device progress (downloading / applying / verifying / complete /
   failed), overall rollout percentage, and can pause/resume/abort.

## Delta Format

### oxide-delta patch format (v1)

```
┌─────────────────────────────────────────┐
│ Header (32 bytes)                       │
│   magic: "OXDL"                         │
│   version: u8                           │
│   base_sha256: [u8; 32]                 │
│   target_sha256: [u8; 32]               │
│   target_size: u64                      │
│   num_chunks: u32                       │
│   compression: u8 (0=none, 1=zstd)     │
├─────────────────────────────────────────┤
│ Chunk 0                                 │
│   offset: u64                           │
│   length: u32                           │
│   type: u8 (0=copy, 1=replace, 2=xor)  │
│   data: [u8; ...] (compressed)          │
├─────────────────────────────────────────┤
│ Chunk 1                                 │
│   ...                                   │
├─────────────────────────────────────────┤
│ ...                                     │
└─────────────────────────────────────────┘
```

**Chunk types:**
- `copy`: this region is identical in base and target — zero bytes in patch
- `replace`: this region is entirely new — raw (compressed) bytes
- `xor`: this region differs — XOR delta (compressed), applied against base

For ONNX-aware mode, chunks align to tensor boundaries (parsed from the
protobuf structure), so unchanged tensors produce `copy` chunks.

### Delta computation

```
fn compute_delta(base: &[u8], target: &[u8]) -> Patch:
    1. Parse both as ONNX protobuf (if possible, fall back to binary)
    2. For each tensor in target:
       a. If tensor exists in base with identical bytes → COPY chunk
       b. If tensor exists but differs → XOR chunk (compressed)
       c. If tensor is new → REPLACE chunk
    3. Handle non-tensor regions (graph structure) as binary diff
    4. Compress each chunk with zstd
    5. Write patch file
```

### Delta application (on device)

```
fn apply_delta(base_path: &Path, patch: &[u8]) -> Result<Vec<u8>>:
    1. Read base file
    2. Verify base_sha256 matches header
    3. Allocate target buffer (target_size from header)
    4. For each chunk:
       - COPY: memcpy from base at offset
       - REPLACE: write chunk data at offset
       - XOR: xor chunk data with base at offset
    5. Verify target_sha256
    6. Return target bytes
```

## Download protocol change

### Current
```
Agent: GET /models/{id}/versions/{ver}/download
Server: 200 OK, body = full model bytes
```

### Proposed
```
Agent: GET /models/{id}/versions/{ver}/download
        X-Oxide-Base-Version: v1.0.0
        X-Oxide-Base-SHA256: abc123...

Server (if delta available):
        200 OK
        Content-Type: application/x-oxide-delta
        X-Oxide-Delta-Base: v1.0.0
        X-Oxide-Target-SHA256: def456...
        body = delta patch bytes

Server (if no delta, or agent has no base):
        200 OK
        Content-Type: application/octet-stream
        X-Oxide-SHA256: def456...
        body = full model bytes
```

The agent sends what it has. The server responds with the smallest
possible payload. Fully backward compatible — old agents without the
header get full files.

## Campaign tracking

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
}

enum DeviceUpdateState {
    Pending,
    Downloading { started_at: DateTime<Utc>, bytes_downloaded: u64 },
    Applying,
    Verifying,
    Complete { completed_at: DateTime<Utc>, delta_bytes: u64 },
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
```

## Health check hooks

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

## Implementation plan

### Phase 1: Delta engine (oxide-delta crate)
- [ ] Binary delta: zstd --patch-from equivalent in pure Rust
- [ ] ONNX parser: extract tensor boundaries from protobuf
- [ ] Tensor-level diff: copy/replace/xor chunks aligned to tensors
- [ ] Patch format: write and read the OXDL format
- [ ] Tests: round-trip with real ONNX models, verify exact reconstruction

### Phase 2: Control plane integration
- [ ] Delta cache: compute and store deltas on model upload
- [ ] Download endpoint: serve delta or full based on agent headers
- [ ] Campaign model: replace fire-and-forget deploy
- [ ] Campaign API: create / status / pause / resume / abort
- [ ] Bandwidth tracking: log bytes served per device per campaign

### Phase 3: Agent updates
- [ ] Delta-aware download: send base version in headers
- [ ] Delta application: reconstruct target from base + patch
- [ ] Health check hooks: call user-supplied command instead of inference
- [ ] Campaign reporting: report download/apply/verify progress per heartbeat
- [ ] Model inventory: track all versions on disk, report to control plane

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

## Success metrics

1. **Delta size**: <15% of full model for typical fine-tuning updates
2. **Round-trip correctness**: SHA-256 of reconstructed model matches
   original — always, no exceptions
3. **Backward compat**: agents without delta support still work
4. **Campaign visibility**: know exactly which devices have which version
   at any point during a rollout
5. **Health check flexibility**: any executable, any language, any runtime

## Open questions

1. **Should we support non-ONNX formats?** SafeTensors (just raw tensors,
   trivial to diff), TFLite (flatbuffer, harder), PyTorch .pt (pickle +
   tensors, messy). Start with ONNX + generic binary fallback.

2. **Should deltas be computed eagerly or lazily?** Eager (on upload) is
   simpler and avoids latency on first download. Lazy saves storage if
   most version pairs are never requested. Start eager.

3. **Multi-hop deltas?** If a device is on v1 and target is v5, should we
   ship v1→v5 directly, or chain v1→v2→v3→v4→v5? Direct is simpler and
   smaller. Chain reuses cached deltas. Start with direct.

4. **Quantized diffs (Level 2)?** If a weight changed from 0.50000 to
   0.50012, do we need the exact delta or is "approximately +0.0001"
   good enough? This is lossy and model-quality-sensitive. Defer to a
   future version with user-configurable tolerance.
