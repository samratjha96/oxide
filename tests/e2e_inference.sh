#!/usr/bin/env bash
# End-to-end test for model inference and security features.
set -e

OXIDE="./target/release/oxide"
MODELS_DIR="models/test"
TMP_DIR=$(mktemp -d)

cleanup() {
    rm -rf "$TMP_DIR"
    rm -rf .oxide
}
trap cleanup EXIT

echo "=== Oxide End-to-End Inference & Security Test ==="
echo ""

# --- Model Info ---
echo "--- Model Info ---"
$OXIDE info "$MODELS_DIR/add_model.onnx" 2>/dev/null
echo "✓ Model info works"

echo ""
echo "--- Model Info (Classifier) ---"
$OXIDE info "$MODELS_DIR/classifier_model.onnx" 2>/dev/null
echo "✓ Classifier model info works"

# --- Inference ---
echo ""
echo "--- Inference: Add Model ---"
OUTPUT=$($OXIDE run "$MODELS_DIR/add_model.onnx" --input "[1.0, 2.0, 3.0, 4.0]" --shape "1,4" 2>/dev/null)
echo "$OUTPUT"
echo "$OUTPUT" | grep -q "\[4.0, 5.0, 6.0, 7.0\]" && echo "✓ Add model output correct" || { echo "✗ Wrong output"; exit 1; }

echo ""
echo "--- Inference: Sigmoid Model ---"
OUTPUT=$($OXIDE run "$MODELS_DIR/sigmoid_model.onnx" --input "[0.0, 0.0, 0.0, 0.0]" --shape "1,4" 2>/dev/null)
echo "$OUTPUT"
echo "$OUTPUT" | grep -q "0.5" && echo "✓ Sigmoid(0) ≈ 0.5" || { echo "✗ Wrong sigmoid output"; exit 1; }

echo ""
echo "--- Inference: Batch Benchmark ---"
$OXIDE bench "$MODELS_DIR/sigmoid_model.onnx" --warmup 10 --iterations 500 2>/dev/null | grep -E "Avg|P50|P99|Throughput|Excellent"
echo "✓ Benchmark works"

# --- Encryption ---
echo ""
echo "--- Encryption ---"
KEY="$TMP_DIR/test.key"
ENC="$TMP_DIR/model.onnx.enc"
DEC="$TMP_DIR/model.onnx.dec"

$OXIDE encrypt "$MODELS_DIR/add_model.onnx" --output "$ENC" --key "$KEY" 2>/dev/null
echo "✓ Model encrypted"
[ -f "$ENC" ] || { echo "✗ Encrypted file not found"; exit 1; }
[ -f "$KEY" ] || { echo "✗ Key file not found"; exit 1; }

$OXIDE decrypt "$ENC" --output "$DEC" --key "$KEY" 2>/dev/null
echo "✓ Model decrypted"

# Verify decrypted model matches original
ORIG_HASH=$(shasum -a 256 "$MODELS_DIR/add_model.onnx" | awk '{print $1}')
DEC_HASH=$(shasum -a 256 "$DEC" | awk '{print $1}')
[ "$ORIG_HASH" = "$DEC_HASH" ] && echo "✓ Decrypted model matches original (SHA-256)" || { echo "✗ Hash mismatch"; exit 1; }

# Run inference on decrypted model
OUTPUT=$($OXIDE run "$DEC" --input "[10.0, 20.0, 30.0, 40.0]" --shape "1,4" 2>/dev/null)
echo "$OUTPUT" | grep -q "\[13.0, 23.0, 33.0, 43.0\]" && echo "✓ Decrypted model inference correct" || { echo "✗ Wrong inference output"; exit 1; }

# --- Encrypted model cannot be loaded directly ---
echo ""
echo "--- Security: Encrypted model is not loadable ---"
if $OXIDE info "$ENC" 2>/dev/null; then
    echo "✗ Encrypted model should not be loadable as ONNX"
    exit 1
else
    echo "✓ Encrypted model correctly rejected"
fi

# --- Device Management ---
echo ""
echo "--- Device Management ---"
rm -rf .oxide
$OXIDE device register rpi-01 --name "Raspberry Pi 1" 2>/dev/null
$OXIDE device register rpi-02 --name "Raspberry Pi 2" 2>/dev/null
$OXIDE device register jetson-01 --name "Jetson Nano 1" 2>/dev/null
$OXIDE device list 2>/dev/null
DEVICE_STATUS=$($OXIDE device status rpi-01 2>/dev/null)
echo "$DEVICE_STATUS" | grep -q "rpi-01" && echo "✓ Device status works" || { echo "✗ Device status failed"; exit 1; }

# --- Binary Size Check ---
echo ""
echo "--- Binary Size ---"
SIZE=$(ls -l "$OXIDE" | awk '{print $5}')
SIZE_MB=$(echo "scale=2; $SIZE / 1048576" | bc)
echo "  Binary size: ${SIZE_MB}MB ($SIZE bytes)"
[ "$SIZE" -lt 10485760 ] && echo "✓ Binary < 10MB target" || { echo "⚠️ Binary exceeds 10MB"; }

echo ""
echo "=== All E2E Inference & Security Tests Passed! ==="
