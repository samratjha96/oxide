# oxide

**Lightweight, secure edge AI runtime for deploying models to resource-constrained devices**

Deploy intelligence at the speed of rust.

---

## Why oxide?

AI inference is moving to the edge—smartphones, IoT sensors, industrial robots, drones. But **70% of edge AI projects stall in pilot phase** because deployment is broken.

| Problem | Today | With oxide |
|---------|-------|------------|
| **Deployment** | Manual, fragile, no orchestration | Fleet management with OTA updates |
| **Resource Usage** | Heavy (500MB+ runtime) | Minimal (<50MB RAM, <10MB binary) |
| **Security** | Models unencrypted, no attestation | Encrypted models, secure boot chain |
| **Updates** | SSH into each device | Push updates to thousands of devices |
| **Monitoring** | Blind (no telemetry) | Real-time metrics, drift detection |
| **Platform** | x86 only | ARM, x86, RISC-V, WebAssembly |

**The gap we fill:** No production-grade solution exists for deploying AI models to edge devices at scale. Companies building edge AI infrastructure (Deepgram, Alignerr) identified this as a critical need.

---

## The Demo

```bash
# Deploy model to single device
oxide deploy face-detection.onnx \
  --device raspberrypi-living-room \
  --quantize int8

# Deploy to entire fleet
oxide deploy speech-to-text.tflite \
  --fleet production-warehouse \
  --rollout canary \
  --health-check "inference_latency < 50ms"

# Monitor fleet health
oxide fleet status --metrics
# Device                  Model Version  Uptime  Latency  Throughput
# pi-warehouse-01        v2.3.1         12d     23ms     45 inf/s
# pi-warehouse-02        v2.3.1         12d     21ms     48 inf/s
# jetson-camera-03       v2.3.1         8d      15ms     120 inf/s
# jetson-camera-04       v2.2.0 ⚠️      45d     18ms     110 inf/s

# Update single device (automatic rollback on failure)
oxide update pi-warehouse-04 --model v2.3.1 --rollback-on-error
```

---

## Architecture

```
                    ┌─────────────────────────────┐
                    │   Oxide Control Plane       │
                    │  ┌──────────┐  ┌──────────┐ │
                    │  │  Fleet   │  │  Model   │ │
                    │  │ Manager  │  │ Registry │ │
                    │  └──────────┘  └──────────┘ │
                    └──────────┬──────────────────┘
                               │
                    ┌──────────┴──────────┐
                    │   Secure Channel    │
                    │  (mTLS, encrypted)  │
                    └──────────┬──────────┘
                               │
        ┌──────────────────────┼──────────────────────┐
        ▼                      ▼                      ▼
┌───────────────┐      ┌───────────────┐      ┌───────────────┐
│   Oxide RT    │      │   Oxide RT    │      │   Oxide RT    │
│ (Raspberry Pi)│      │ (Jetson Nano) │      │  (Intel NUC)  │
├───────────────┤      ├───────────────┤      ├───────────────┤
│ Model Loader  │      │ Model Loader  │      │ Model Loader  │
│ Inference Eng │      │ Inference Eng │      │ Inference Eng │
│ Telemetry     │      │ Telemetry     │      │ Telemetry     │
│ OTA Updater   │      │ OTA Updater   │      │ OTA Updater   │
└───────────────┘      └───────────────┘      └───────────────┘
```

### Runtime Features

| Feature | Details |
|---------|---------|
| **Model Formats** | ONNX, TensorFlow Lite, CoreML, GGUF |
| **Inference** | CPU-optimized (quantization-aware, SIMD) |
| **Memory** | <50MB RAM for runtime + model |
| **Binary Size** | <10MB (static, no dependencies) |
| **Startup** | <1 second cold start |
| **Security** | Encrypted models, secure boot attestation |
| **Updates** | OTA with versioning, rollback, health checks |
| **Telemetry** | Lightweight metrics (latency, throughput, drift) |

### Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| ARM (Raspberry Pi, Jetson) | Primary | ARMv7, ARMv8, aarch64 |
| x86 (Intel NUC, embedded PC) | Primary | Static binary, no runtime deps |
| RISC-V | Planned | Cross-compile support |
| WebAssembly | Planned | Browser/edge function execution |

---

## Benchmarks (Targets)

Measured on Raspberry Pi 4 (4GB RAM, quad-core ARM Cortex-A72):

| Metric | Target | Why It Matters |
|--------|--------|----------------|
| Cold start | <1s | Device reboots shouldn't cause downtime |
| Runtime memory | <50MB | Leave room for application logic |
| Binary size | <10MB | Fast OTA updates over cellular/satellite |
| Inference latency | <10ms | Real-time applications (video, audio) |
| Model load time | <500ms | Hot-swapping models for multi-task devices |
| Update bandwidth | <5MB | Work on low-bandwidth networks |

---

## Real-World Use Cases

### 1. Industrial Quality Control
- **Problem:** 1000 cameras inspecting parts on assembly line
- **Solution:** Deploy defect-detection model to all cameras simultaneously
- **Benefit:** Catch defects in real-time, no cloud latency, works when offline

### 2. Smart Cities
- **Problem:** 500 traffic cameras, need to update pedestrian detection model
- **Solution:** Canary rollout - 5% → 25% → 100% with automatic rollback
- **Benefit:** Test model in production safely, rollback bad updates instantly

### 3. Agricultural Drones
- **Problem:** 200 drones identifying crop diseases, models improve weekly
- **Solution:** Push model updates over cellular when drones land
- **Benefit:** Always running latest models, minimal downtime

### 4. Retail Analytics
- **Problem:** 50 stores with edge devices for customer tracking
- **Solution:** Monitor inference drift, A/B test new models per location
- **Benefit:** Detect when model quality degrades, optimize per store

---

## Implementation

### Crate Structure

```
oxide/
├── crates/
│   ├── oxide-core/           # Core traits, types, config
│   ├── oxide-runtime/        # Inference engine, model loading
│   ├── oxide-network/        # mTLS, OTA updates, telemetry
│   ├── oxide-models/         # Model format parsers (ONNX, TFLite)
│   ├── oxide-security/       # Encryption, attestation
│   ├── oxide-control/        # Control plane (fleet management)
│   └── oxide-cli/            # CLI for deployment, monitoring
├── oxide-py/                 # Python SDK for model preparation
├── models/                   # Example models
│   ├── object-detection/
│   ├── speech-to-text/
│   └── image-classification/
├── docs/                     # Documentation
│   ├── research/             # Research findings
│   ├── guides/               # How-to guides
│   └── api/                  # API documentation
└── examples/                 # Example deployments
    ├── raspberry-pi/
    ├── jetson-nano/
    └── industrial-camera/
```

### Key Design Decisions

1. **CPU-first inference** - No GPU required
   - Quantization-aware execution (int8, fp16)
   - SIMD acceleration (NEON, AVX)
   - Model optimization (graph fusion, constant folding)

2. **Security by default**
   - Models encrypted at rest and in transit
   - Secure boot chain verification
   - mTLS for all device ↔ control plane communication
   - Attestation before model loading

3. **Resilient updates**
   - Atomic model updates (all-or-nothing)
   - Health checks before marking update successful
   - Automatic rollback on failure
   - Canary rollouts (staged deployment)

4. **Offline-first**
   - Devices work when disconnected
   - Queue telemetry for later sync
   - Local fallback policies
   - Eventual consistency

---

## Supported Model Types

| Model Format | Status | Use Cases |
|--------------|--------|-----------|
| ONNX | Primary | PyTorch, TensorFlow exports |
| TensorFlow Lite | Primary | Mobile models, quantization |
| CoreML | Planned | Apple ecosystem models |
| GGUF | Planned | LLM inference (llama.cpp format) |
| Custom | Extensible | Bring your own format |

---

## Phases

### Phase 1: Core Runtime (4 weeks)
- [ ] oxide-core traits and types
- [ ] oxide-runtime inference engine
- [ ] ONNX model loading
- [ ] Quantization support (int8)
- [ ] CPU inference with SIMD
- **Exit**: `oxide run face-detection.onnx --input image.jpg` works on Pi

### Phase 2: Security + Updates (3 weeks)
- [ ] Model encryption/decryption
- [ ] Secure boot attestation
- [ ] OTA update mechanism
- [ ] Rollback logic
- **Exit**: Secure model loading, atomic updates work

### Phase 3: Fleet Management (4 weeks)
- [ ] oxide-control control plane
- [ ] Fleet registry and device management
- [ ] Telemetry collection and aggregation
- [ ] Canary rollout strategy
- **Exit**: Deploy model to 10 devices simultaneously

### Phase 4: Production Polish (3 weeks)
- [ ] Comprehensive benchmarks
- [ ] Documentation and guides
- [ ] Example deployments (Pi, Jetson, x86)
- [ ] Python SDK for model preparation
- **Exit**: Production-ready, documented, benchmarked

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Inference too slow on CPU | Quantization, SIMD, graph optimization |
| Binary too large | Static linking, strip symbols, minimal deps |
| Security complexity | Use battle-tested libraries (rustls, ring) |
| Cross-compilation issues | Extensive CI/CD for all target platforms |
| Model format support | Start with ONNX (80% coverage), add others later |
| Network unreliability | Offline-first design, retry logic, queuing |

---

## Research

Detailed research findings compiled from web research in `docs/research/`:

- [01-edge-ai-deployment-challenges.md](docs/research/01-edge-ai-deployment-challenges.md) - Why 70% of projects fail
- [02-rust-for-edge-inference.md](docs/research/02-rust-for-edge-inference.md) - Why Rust is ideal for edge
- [03-model-optimization-techniques.md](docs/research/03-model-optimization-techniques.md) - Quantization, pruning, distillation
- [04-secure-ota-updates.md](docs/research/04-secure-ota-updates.md) - Update mechanisms and security
- [05-fleet-management-patterns.md](docs/research/05-fleet-management-patterns.md) - Orchestration at scale
- [06-inference-engine-comparison.md](docs/research/06-inference-engine-comparison.md) - ONNX Runtime vs TFLite vs custom
- [07-real-world-edge-deployments.md](docs/research/07-real-world-edge-deployments.md) - Case studies from industry
- [08-nvidia-spectrum-x-networking.md](docs/research/08-nvidia-spectrum-x-networking.md) - Network telemetry research
- [09-cloudflare-infire-analysis.md](docs/research/09-cloudflare-infire-analysis.md) - Inference engine optimization
- [10-production-rust-ai-projects.md](docs/research/10-production-rust-ai-projects.md) - What companies are building

---

## Quick Reference

```bash
# Build (requires Rust toolchain)
cargo build --release --workspace

# Cross-compile for Raspberry Pi
cross build --release --target armv7-unknown-linux-gnueabihf

# Run inference locally
oxide run model.onnx --input data.json

# Deploy to device
oxide deploy model.onnx --device pi-camera-01

# Deploy to fleet
oxide deploy model.onnx --fleet production --rollout canary

# Monitor fleet
oxide fleet status
oxide fleet metrics --device pi-camera-01

# Update device
oxide update pi-camera-01 --model v2.0.0

# Rollback
oxide rollback pi-camera-01 --to-version v1.9.5

# Development
cargo test --workspace
cargo bench
cargo doc --open
```

---

## Why "Oxide"?

**Oxide = Iron Oxide = Rust**

The name reflects:
- **Chemistry tie-in**: Rust programming language (iron oxide)
- **Edge deployment**: "Oxidize your edge devices"
- **Stability**: Like rust, it's a stable, protective layer
- **Modern**: Fits with modern tech naming (Oxide Computer, Vercel, Supabase)

Tagline: *"Deploy intelligence at the speed of rust"*

---

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Adoption | 100 GitHub stars in 3 months | Community interest |
| Performance | <50MB RAM, <1s startup on Pi 4 | Benchmarks |
| Reliability | 99.9% update success rate | Telemetry |
| Documentation | <15min to first deployment | User testing |
| Compatibility | 5+ model formats, 4+ platforms | Feature matrix |

---

## Related Projects

Learn from and differentiate against:

| Project | Focus | Oxide Advantage |
|---------|-------|-----------------|
| **TensorFlow Lite** | Mobile inference | Better fleet management, security |
| **ONNX Runtime** | Cross-platform inference | Lighter weight, edge-optimized |
| **AWS IoT Greengrass** | Cloud-connected edge | Simpler, no vendor lock-in |
| **Azure IoT Edge** | Enterprise edge | Open source, no cloud required |
| **llama.cpp** | LLM inference | Broader model support, fleet management |

---

## Getting Started

### For Developers
```bash
git clone https://github.com/your-username/oxide
cd oxide
cargo build --release
cargo test
./target/release/oxide --help
```

### For Device Operators
```bash
# Install (cross-platform binary)
curl -sSL https://oxide.dev/install.sh | sh

# Deploy your first model
oxide deploy model.onnx --device my-device
```

### For Model Engineers
```python
# Optimize and export model for oxide
from oxide import optimize

model = optimize(
    "model.pytorch",
    format="onnx",
    quantize="int8",
    target="raspberry-pi"
)
model.save("model-optimized.onnx")
```

---

## Contributing

This is an ambitious project addressing a real gap in edge AI infrastructure. Contributions welcome in:

- Core runtime optimization
- Model format support
- Platform support (new architectures)
- Security hardening
- Documentation and examples
- Testing and benchmarking

See `CONTRIBUTING.md` for guidelines.

---

## License

Apache 2.0 or MIT (dual-licensed for maximum compatibility)

---

## Contact & Community

- **GitHub**: [oxide-edge-ai](https://github.com/your-username/oxide)
- **Docs**: [oxide.dev/docs](https://oxide.dev/docs)
- **Discussions**: [GitHub Discussions](https://github.com/your-username/oxide/discussions)
- **Discord**: [Join our community](https://discord.gg/oxide)

Built with 🦀 by the Rust community for the edge AI future.
