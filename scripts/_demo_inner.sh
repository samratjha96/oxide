#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# Inner demo script — runs inside the narrator tmux pane.
# Orchestrates the full OTA lifecycle while the user watches
# all panes update in real time.
#
# Expects env vars: DEMO_PANE_CP, DEMO_PANE_A1..A3
# ──────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
OXIDE="$PROJECT_DIR/target/release/oxide"
SESSION="oxide-demo"
PORT=19080
BASE="http://127.0.0.1:$PORT"

MODEL_V1="$PROJECT_DIR/models/test/mlp_mnist.onnx"       # 2 MB, 535K params
MODEL_V2="$PROJECT_DIR/models/test/classifier_model.onnx" # 1 KB, different arch

# Pane IDs from outer script
CP="$DEMO_PANE_CP"
A1="$DEMO_PANE_A1"
A2="$DEMO_PANE_A2"
A3="$DEMO_PANE_A3"
AGENTS=("$A1" "$A2" "$A3")

# ── Formatting ────────────────────────────────────────────────

B="\033[1m"     # bold
D="\033[2m"     # dim
G="\033[32m"    # green
C="\033[36m"    # cyan
R="\033[0m"     # reset

step() { echo ""; echo -e "${B}${G}━━━ $1 ━━━${R}"; echo ""; }
say()  { echo -e "${B}${C}▸${R} $1"; }
dim()  { echo -e "${D}  $1${R}"; }

wait_key() {
    echo ""
    echo -e "${D}  ⏎  press Enter to continue${R}"
    read -r
}

wait_for_server() {
    local n=0
    while ! curl -sf "$BASE/health" &>/dev/null; do
        n=$((n + 1)); [[ $n -gt 30 ]] && { echo "Server didn't start"; exit 1; }
        sleep 0.3
    done
}

wait_for_agent() {
    local dir="$1" ver="$2" timeout="${3:-60}" n=0
    while [[ $n -lt $timeout ]]; do
        if [[ -f "$dir/.agent-state.json" ]] &&
           grep -q "\"current_model_version\": \"$ver\"" "$dir/.agent-state.json" 2>/dev/null; then
            return 0
        fi
        sleep 0.5; n=$((n + 1))
    done
    echo "  ✗ timeout waiting for $ver in $dir"
    return 1
}

# ── Title ─────────────────────────────────────────────────────

clear
cat <<'BANNER'

   ██████  ██   ██ ██ ██████  ███████
  ██    ██  ██ ██  ██ ██   ██ ██
  ██    ██   ███   ██ ██   ██ █████
  ██    ██  ██ ██  ██ ██   ██ ██
   ██████  ██   ██ ██ ██████  ███████

  Fleet OTA Demo
BANNER
echo ""
say "This demo runs a control plane + 3 device agents."
say "You'll watch a model get deployed, then OTA-updated."
echo ""
dim "→  right pane: control plane HTTP server"
dim "→  bottom panes: 3 edge device agents"
dim "→  this pane: orchestrator narration"
wait_key

# ── Act 1: Control Plane ──────────────────────────────────────

step "1 · START CONTROL PLANE"

tmux send-keys -t "$CP" "cd $PROJECT_DIR && $OXIDE serve --port $PORT" Enter
wait_for_server

say "Control plane is live"
dim "$(curl -sf "$BASE/health" | jq -c .)"
wait_key

# ── Act 2: Register Devices & Fleet ──────────────────────────

step "2 · REGISTER DEVICES & CREATE FLEET"

for i in 1 2 3; do
    curl -sf -X POST "$BASE/api/v1/devices" \
        -H "Content-Type: application/json" \
        -d "{\"id\": \"edge-$i\", \"name\": \"Sensor $i\"}" >/dev/null
    say "Registered edge-$i"
done

echo ""
curl -sf -X POST "$BASE/api/v1/fleets" \
    -H "Content-Type: application/json" \
    -d '{"id": "factory", "name": "Factory Floor"}' >/dev/null

for i in 1 2 3; do
    curl -sf -X POST "$BASE/api/v1/fleets/factory/devices/edge-$i" >/dev/null
done

say "Fleet 'factory' → 3 devices"
echo ""
curl -sf "$BASE/api/v1/devices" | jq -r '.[] | "    \(.id)  \(.name)  [\(.status)]"'
wait_key

# ── Act 3: Upload Model v1 ───────────────────────────────────

step "3 · UPLOAD MODEL v1"

say "Uploading mlp_mnist.onnx — 535K params, 2 MB..."
RESP=$(curl -sf -X POST "$BASE/api/v1/models/digit-detect/versions/v1.0.0" \
    --data-binary @"$MODEL_V1")

say "Stored: digit-detect@v1.0.0"
dim "size: $(echo "$RESP" | jq -r .size_bytes) bytes"
dim "sha256: $(echo "$RESP" | jq -r '.sha256[0:16]')..."
wait_key

# ── Act 4: Deploy v1 ─────────────────────────────────────────

step "4 · DEPLOY v1 TO FLEET"

say "Assigning digit-detect@v1.0.0 to all 3 devices..."
RESP=$(curl -sf -X POST "$BASE/api/v1/fleets/factory/deploy" \
    -H "Content-Type: application/json" \
    -d '{"model_id": "digit-detect", "model_version": "v1.0.0", "strategy": "all_at_once"}')

dim "$(echo "$RESP" | jq -c .)"
say "Devices will see the assignment on their next heartbeat"
wait_key

# ── Act 5: Start Agents ──────────────────────────────────────

step "5 · START AGENT DAEMONS"

say "Launching 3 agents — watch the bottom panes ↓"
echo ""

for i in 1 2 3; do
    IDX=$((i - 1))
    tmux send-keys -t "${AGENTS[$IDX]}" \
        "$OXIDE agent --control-plane $BASE --device-id edge-$i --poll-interval 5 --model-dir /tmp/oxide-demo-agent-$i" Enter
    sleep 0.3
done

say "Waiting for all 3 to pick up v1.0.0..."
echo ""

for i in 1 2 3; do
    wait_for_agent "/tmp/oxide-demo-agent-$i" "v1.0.0"
    say "  ✓ edge-$i → digit-detect@v1.0.0"
done
wait_key

# ── Act 6: Verify ────────────────────────────────────────────

step "6 · VERIFY FLEET STATE"

for i in 1 2 3; do
    STATE=$(cat "/tmp/oxide-demo-agent-$i/.agent-state.json")
    say "  edge-$i: $(echo "$STATE" | jq -r '"\(.current_model)@\(.current_model_version)"')"
done

echo ""
say "All 3 devices running the same model ✓"
wait_key

# ── Act 7: OTA Update ────────────────────────────────────────

step "7 · OTA UPDATE: v1 → v2"

say "Uploading classifier_model.onnx (new architecture, 1 KB)..."
RESP=$(curl -sf -X POST "$BASE/api/v1/models/digit-detect/versions/v2.0.0" \
    --data-binary @"$MODEL_V2")
dim "size: $(echo "$RESP" | jq -r .size_bytes) bytes"
echo ""

say "Deploying v2.0.0 → fleet-wide OTA..."
curl -sf -X POST "$BASE/api/v1/fleets/factory/deploy" \
    -H "Content-Type: application/json" \
    -d '{"model_id": "digit-detect", "model_version": "v2.0.0", "strategy": "all_at_once"}' >/dev/null

echo ""
say "Watch: backup v1 → download v2 → SHA-256 → swap → health check ↓"
echo ""

for i in 1 2 3; do
    wait_for_agent "/tmp/oxide-demo-agent-$i" "v2.0.0"
    say "  ✓ edge-$i updated to v2.0.0"
done
wait_key

# ── Act 8: Final State ───────────────────────────────────────

step "8 · FINAL STATE"

say "Devices:"
for i in 1 2 3; do
    STATE=$(cat "/tmp/oxide-demo-agent-$i/.agent-state.json")
    say "  edge-$i: $(echo "$STATE" | jq -r '"\(.current_model)@\(.current_model_version)"')  updated $(echo "$STATE" | jq -r '.last_update')"
done

echo ""
say "Model store:"
curl -sf "$BASE/api/v1/models/digit-detect" | \
    jq -r '.versions[] | "    \(.version)  \(.size_bytes) bytes  \(.sha256[0:12])..."'

echo ""
say "Disk (agent 1):"
find /tmp/oxide-demo-agent-1 -type f -not -name '.DS_Store' | sort | while read -r f; do
    SIZE=$(wc -c < "$f" | tr -d ' ')
    dim "$(echo "$f" | sed 's|/tmp/oxide-demo-agent-1/||')  ($SIZE bytes)"
done

# ── Fin ───────────────────────────────────────────────────────

step "DONE"

echo ""
say "Recap:"
say "  • 1 control plane, 3 agent daemons — all real processes"
say "  • Model v1 (535K params, 2 MB) deployed via OTA to 3 devices"
say "  • Model v2 pushed → fleet-wide update with v1 backup"
say "  • Each agent: download → SHA-256 → atomic swap → live ONNX inference"
say "  • One ${B}6 MB binary${R}. No Python. No Docker. No cloud."
echo ""
echo -e "${D}  Press Enter to tear down, or Ctrl-C to keep exploring.${R}"
read -r

# Graceful teardown
for pane in "${AGENTS[@]}"; do
    tmux send-keys -t "$pane" C-c
    sleep 0.3
done
tmux send-keys -t "$CP" C-c
sleep 1
tmux kill-session -t "$SESSION" 2>/dev/null || true
