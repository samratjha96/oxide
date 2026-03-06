# Stage 1: Build Oxide from source
# Uses the official Rust image on the same arch as the host (arm64 on Apple Silicon)
FROM rust:latest AS builder

WORKDIR /build
COPY . .

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Release build: LTO, stripped, optimized for size
RUN cargo build --release -p oxide-cli 2>&1 \
    && strip target/release/oxide

# Stage 2: Minimal runtime image simulating a constrained edge device
# debian-slim: ~80MB, has libc, mimics a real embedded Linux (Pi OS, Jetson, etc.)
FROM debian:bookworm-slim

# Simulate device identity
ENV OXIDE_DEVICE_ID="edge-sensor-01"
ENV OXIDE_DEVICE_NAME="Assembly Line Camera"

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user (realistic for edge devices)
RUN useradd --create-home --shell /bin/bash oxide
USER oxide
WORKDIR /home/oxide

# Copy only the binary — nothing else
COPY --from=builder /build/target/release/oxide /usr/local/bin/oxide

# Create working directories
RUN mkdir -p models .oxide

# Copy test models in
COPY --chown=oxide:oxide models/test/classifier_model.onnx models/classifier.onnx
COPY --chown=oxide:oxide models/test/mlp_mnist.onnx models/mlp_mnist.onnx

ENTRYPOINT ["oxide"]
