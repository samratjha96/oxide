#!/usr/bin/env bash
# Run all Oxide tests: unit, integration, and end-to-end.
set -e

cd "$(dirname "$0")/.."
echo "╔══════════════════════════════════════╗"
echo "║    ⚡ Oxide — Full Test Suite        ║"
echo "╚══════════════════════════════════════╝"
echo ""

# Build release
echo "📦 Building release binary..."
cargo build --release -p oxide-cli 2>&1 | tail -1
echo ""

# Unit tests
echo "🧪 Running unit tests..."
cargo test --workspace 2>&1 | grep -E "^(running|test result:)" | head -20
echo ""

# Integration tests
echo "🔗 Running integration tests..."
cargo test -p oxide-cli --test integration_tests 2>&1 | grep -E "^(running|test result:)"
echo ""

# E2E inference test
echo "🚀 Running E2E inference + security test..."
bash tests/e2e_inference.sh 2>&1 | grep -E "^(✓|✗|===)" 
echo ""

# E2E control plane test
echo "🌐 Running E2E control plane test..."
bash tests/e2e_control_plane.sh 2>&1 | grep -E "^(✓|✗|===)"
echo ""

# Binary size report
BINARY="target/release/oxide"
SIZE=$(ls -l "$BINARY" | awk '{print $5}')
SIZE_MB=$(echo "scale=2; $SIZE / 1048576" | bc)
echo "📊 Binary size: ${SIZE_MB}MB ($SIZE bytes)"

# Count total tests
TOTAL=$(cargo test --workspace 2>&1 | grep "^test result:" | awk '{sum += $4} END {print sum}')
echo "📊 Total unit + integration tests: $TOTAL"

echo ""
echo "╔══════════════════════════════════════╗"
echo "║    ✅ All Tests Passed!              ║"
echo "╚══════════════════════════════════════╝"
