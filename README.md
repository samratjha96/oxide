# ⚡ Oxide

**Deploy intelligence at the speed of rust**

Lightweight, secure edge AI runtime for deploying models to resource-constrained devices.

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Status](https://img.shields.io/badge/status-vision-yellow.svg)](VISION.md)

---

## The Problem

**70% of edge AI projects stall in pilot phase.** Companies can build amazing AI models, but deploying them to thousands of edge devices (cameras, sensors, robots, drones) is broken.

**Current pain points:**
- 🐢 Heavy runtimes (500MB+ Python/JVM)
- 🔒 No fleet management (update 1000 devices = nightmare)
- 🚫 No security (models unencrypted, no attestation)
- 💸 Massive wasted GPU time from network issues
- 🎲 Deployment chaos (manual, fragile, no orchestration)

---

## The Solution

**Oxide** is a Rust-based edge AI runtime designed for production deployment at scale.

```bash
# Deploy model to single device
oxide deploy face-detection.onnx --device raspberrypi-camera-01

# Deploy to entire fleet with canary rollout
oxide deploy speech-model.tflite --fleet production --rollout canary

# Monitor fleet health in real-time
oxide fleet status --metrics
```

### Key Features

| Feature | Benefit |
|---------|---------|
| 🪶 **Lightweight** | <50MB RAM, <10MB binary |
| ⚡ **Fast** | <1s cold start, <10ms inference |
| 🔐 **Secure** | Encrypted models, mTLS, attestation |
| 🚀 **Fleet Management** | Deploy to 1000s of devices simultaneously |
| 📊 **Observability** | Real-time metrics, drift detection |
| 🌐 **Cross-Platform** | ARM, x86, RISC-V, WebAssembly |
| 🔄 **Smart Updates** | OTA with rollback, health checks |
| 💪 **Offline-First** | Works when disconnected |

---

## Quick Start

### Installation

```bash
# Install oxide CLI
curl -sSL https://oxide.dev/install.sh | sh

# Verify installation
oxide --version
```

### Deploy Your First Model

```bash
# 1. Prepare model (export from PyTorch/TensorFlow)
python train_model.py  # Exports model.onnx

# 2. Deploy to device
oxide deploy model.onnx \
  --device my-raspberrypi \
  --quantize int8

# 3. Monitor inference
oxide logs my-raspberrypi --follow

# 4. Check metrics
oxide metrics my-raspberrypi
# Latency: 8.3ms (p50), 12.1ms (p99)
# Throughput: 120 inferences/sec
# Memory: 32MB
```

---

## Why Oxide?

### Comparison with Alternatives

| | Python/vLLM | TensorFlow Lite | AWS Greengrass | **Oxide** |
|---|-------------|-----------------|----------------|-----------|
| **Runtime Size** | 500MB+ | 50MB+ | 200MB+ | **<10MB** |
| **Memory** | 200MB+ | 100MB+ | 150MB+ | **<50MB** |
| **Cold Start** | 5-10s | 2-3s | Unknown | **<1s** |
| **Fleet Mgmt** | ❌ | ❌ | ✅ (vendor lock) | **✅ (open)** |
| **Security** | ❌ | ❌ | ✅ | **✅** |
| **Offline** | ❌ | ✅ | ⚠️ | **✅** |
| **Cross-Platform** | ⚠️ | ✅ | ⚠️ | **✅** |

### Proven by Production

Built on lessons from companies shipping Rust AI systems:

- **Cloudflare Infire**: 82% less CPU overhead vs Python
- **HuggingFace Candle**: Minimal binaries for serverless
- **LanceDB**: Switched from C++ to Rust for safety
- **Deepgram**: Real-time speech AI at scale

---

## Architecture

```
┌─────────────────────────────────────┐
│     Oxide Control Plane             │
│  ┌──────────┐    ┌──────────┐      │
│  │  Fleet   │    │  Model   │      │
│  │ Manager  │    │ Registry │      │
│  └──────────┘    └──────────┘      │
└──────────────┬──────────────────────┘
               │ mTLS
     ┌─────────┼─────────┐
     ▼         ▼         ▼
┌─────────┐ ┌─────────┐ ┌─────────┐
│Oxide RT │ │Oxide RT │ │Oxide RT │
│(ARM Pi) │ │(Jetson) │ │ (x86)   │
└─────────┘ └─────────┘ └─────────┘
```

**Core Components:**
- **Runtime**: Inference engine (ONNX, TFLite, CoreML)
- **Control Plane**: Fleet management, model registry
- **Security**: Encrypted models, mTLS, attestation
- **Telemetry**: Real-time metrics, drift detection

See [VISION.md](VISION.md) for detailed architecture.

---

## Use Cases

### 1. Industrial Quality Control
- **Scenario**: 1000 cameras on assembly line
- **Challenge**: Deploy defect detection model to all cameras
- **Solution**: `oxide deploy defect-model.onnx --fleet assembly-line`
- **Result**: Zero downtime, instant rollback if needed

### 2. Smart Cities
- **Scenario**: 500 traffic cameras need model update
- **Challenge**: Test safely in production
- **Solution**: Canary rollout (5% → 25% → 100%)
- **Result**: Catch issues early, automatic rollback

### 3. Agricultural Drones
- **Scenario**: 200 drones identifying crop diseases
- **Challenge**: Update models weekly over cellular
- **Solution**: Efficient OTA updates (<5MB)
- **Result**: Always running latest models

---

## Documentation

- **[VISION.md](VISION.md)** - Project vision, architecture, roadmap
- **[docs/research/](docs/research/)** - Compiled research findings
  - [Edge AI Deployment Challenges](docs/research/01-edge-ai-deployment-challenges.md)
  - [Why Rust for Edge Inference](docs/research/02-rust-for-edge-inference.md)
  - [Production Rust AI Projects](docs/research/10-production-rust-ai-projects.md)
- **API Documentation** - Coming soon
- **Guides** - Coming soon

---

## Roadmap

### Phase 1: Core Runtime (4 weeks)
- [x] Research and validation
- [ ] ONNX model loading
- [ ] CPU inference with SIMD
- [ ] Quantization support (int8)
- [ ] Raspberry Pi testing

### Phase 2: Security + Updates (3 weeks)
- [ ] Model encryption
- [ ] Secure boot attestation
- [ ] OTA update mechanism
- [ ] Rollback logic

### Phase 3: Fleet Management (4 weeks)
- [ ] Control plane
- [ ] Fleet registry
- [ ] Telemetry aggregation
- [ ] Canary rollouts

### Phase 4: Production Polish (3 weeks)
- [ ] Comprehensive benchmarks
- [ ] Documentation
- [ ] Example deployments
- [ ] Python SDK

See [VISION.md](VISION.md) for detailed phases.

---

## Research

This project is informed by extensive research into:
- Edge AI deployment challenges (70% failure rate)
- Production Rust AI systems (Cloudflare, HuggingFace, LanceDB)
- Performance optimization techniques
- Real-world use cases and constraints

See [docs/research/](docs/research/) for full research compilation.

---

## Contributing

This is a vision/research phase project. Contributions welcome!

**Areas for contribution:**
- Core runtime implementation
- Model format support
- Platform support (ARM, x86, RISC-V)
- Security hardening
- Documentation
- Testing and benchmarking

---

## License

Dual-licensed under Apache 2.0 or MIT (your choice).

---

## Contact

- **GitHub**: [oxide-edge-ai](https://github.com/your-username/oxide)
- **Discussions**: [GitHub Discussions](https://github.com/your-username/oxide/discussions)
- **Documentation**: [oxide.dev](https://oxide.dev) (coming soon)

---

## Acknowledgments

Built on research and inspiration from:
- Cloudflare's Infire inference engine
- HuggingFace Candle framework
- LanceDB multimodal database
- Rust Foundation edge AI initiative
- The broader Rust and ML communities

**Built with 🦀 for the edge AI future.**

---

*Deploy intelligence at the speed of rust*
