#!/usr/bin/env bash
# End-to-end test for the Oxide control plane server.
# Starts the server, registers devices, creates fleets, and deploys models.
set -e

OXIDE="./target/release/oxide"
PORT=18090
BASE="http://127.0.0.1:$PORT"

# Clean state
rm -rf .oxide

echo "=== Oxide End-to-End Control Plane Test ==="
echo ""

# Start server in background
$OXIDE serve --port $PORT &
SERVER_PID=$!
sleep 1

cleanup() {
    kill $SERVER_PID 2>/dev/null || true
    rm -rf .oxide
}
trap cleanup EXIT

echo "✓ Server started (PID $SERVER_PID)"

# Health check
echo ""
echo "--- Health Check ---"
HEALTH=$(curl -s "$BASE/health")
echo "$HEALTH" | python3 -m json.tool
STATUS=$(echo "$HEALTH" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])")
[ "$STATUS" = "healthy" ] && echo "✓ Health check passed" || { echo "✗ Health check failed"; exit 1; }

# Register devices
echo ""
echo "--- Register Devices ---"
for i in $(seq 1 5); do
    RESP=$(curl -s -X POST "$BASE/api/v1/devices" \
        -H "Content-Type: application/json" \
        -d "{\"id\": \"pi-cam-$i\", \"name\": \"Camera $i\"}")
    echo "  Registered: $RESP"
done
echo "✓ 5 devices registered"

# List devices
echo ""
echo "--- List Devices ---"
DEVICES=$(curl -s "$BASE/api/v1/devices")
COUNT=$(echo "$DEVICES" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
echo "  Device count: $COUNT"
[ "$COUNT" = "5" ] && echo "✓ Correct device count" || { echo "✗ Wrong count: $COUNT"; exit 1; }

# Get single device
echo ""
echo "--- Get Device ---"
DEVICE=$(curl -s "$BASE/api/v1/devices/pi-cam-1")
echo "$DEVICE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'  ID: {d[\"id\"]}, Name: {d[\"name\"]}, Status: {d[\"status\"]}')"

# Heartbeat
echo ""
echo "--- Device Heartbeat ---"
curl -s -X POST "$BASE/api/v1/devices/pi-cam-1/heartbeat" | python3 -m json.tool
echo "✓ Heartbeat sent"

# Create fleet
echo ""
echo "--- Create Fleet ---"
FLEET_RESP=$(curl -s -X POST "$BASE/api/v1/fleets" \
    -H "Content-Type: application/json" \
    -d '{"id": "warehouse", "name": "Warehouse Cameras", "description": "Quality inspection cameras"}')
echo "  $FLEET_RESP"
echo "✓ Fleet created"

# Add devices to fleet
echo ""
echo "--- Add Devices to Fleet ---"
for i in $(seq 1 5); do
    curl -s -X POST "$BASE/api/v1/fleets/warehouse/devices/pi-cam-$i" > /dev/null
done
echo "✓ 5 devices added to fleet"

# Fleet status
echo ""
echo "--- Fleet Status ---"
STATUS=$(curl -s "$BASE/api/v1/fleets/warehouse/status")
echo "$STATUS" | python3 -m json.tool
TOTAL=$(echo "$STATUS" | python3 -c "import sys,json; print(json.load(sys.stdin)['total_devices'])")
[ "$TOTAL" = "5" ] && echo "✓ Fleet has 5 devices" || { echo "✗ Wrong count: $TOTAL"; exit 1; }

# Deploy to fleet
echo ""
echo "--- Deploy to Fleet ---"
DEPLOY=$(curl -s -X POST "$BASE/api/v1/fleets/warehouse/deploy" \
    -H "Content-Type: application/json" \
    -d '{"model_id": "defect-detection", "model_version": "v3.0.0", "strategy": "all_at_once"}')
echo "$DEPLOY" | python3 -m json.tool
SUCCESSFUL=$(echo "$DEPLOY" | python3 -c "import sys,json; print(json.load(sys.stdin)['successful'])")
echo "  Successful deployments: $SUCCESSFUL"

# Unregister a device
echo ""
echo "--- Unregister Device ---"
curl -s -X DELETE "$BASE/api/v1/devices/pi-cam-5" | python3 -m json.tool
echo "✓ Device pi-cam-5 unregistered"

# Verify device count
DEVICES=$(curl -s "$BASE/api/v1/devices")
COUNT=$(echo "$DEVICES" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
[ "$COUNT" = "4" ] && echo "✓ 4 devices remaining" || { echo "✗ Wrong count: $COUNT"; exit 1; }

echo ""
echo "=== All E2E Tests Passed! ==="
