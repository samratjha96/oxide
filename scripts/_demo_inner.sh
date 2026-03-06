#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# Inner demo script — runs inside the narrator tmux pane.
# Orchestrates the full Oxide OTA lifecycle.
#
# Expects env vars: DEMO_PANE_CP, DEMO_PANE_A1..A3
# ──────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
OXIDE="$PROJECT_DIR/target/release/oxide"
PORT=19080
BASE="http://127.0.0.1:$PORT"

MODEL_V1="$PROJECT_DIR/models/test/mlp_mnist.onnx"
MODEL_V2="$PROJECT_DIR/models/test/mlp_mnist_v2.onnx"

CP="$DEMO_PANE_CP"
A1="$DEMO_PANE_A1"
A2="$DEMO_PANE_A2"
A3="$DEMO_PANE_A3"
AGENTS=("$A1" "$A2" "$A3")

# ── Formatting ────────────────────────────────────────────────

B="\033[1m"
D="\033[2m"
G="\033[32m"
C="\033[36m"
Y="\033[33m"
R="\033[0m"

step() { echo ""; echo -e "${B}${G}━━━ $1 ━━━${R}"; echo ""; }
say()  { echo -e "${B}${C}▸${R} $1"; }
dim()  { echo -e "${D}  $1${R}"; }
hi()   { echo -e "${B}${Y}  ★ $1${R}"; }

wait_key() {
    echo ""
    echo -e "${D}  press Enter to continue${R}"
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
    echo "  timeout waiting for $ver in $dir"
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

  ML Model Delivery — Fleet OTA Demo
BANNER
echo ""
say "Oxide ships ML model updates to device fleets."
say "When only a few layers change, it ships only the diff."
echo ""
dim "right pane:   control plane (model store + fleet registry)"
dim "bottom panes: 3 edge device agents polling for updates"
dim "this pane:    orchestrator"
wait_key

# ── Act 1: Control Plane ──────────────────────────────────────

step "1 · START CONTROL PLANE"

tmux send-keys -t "$CP" "cd $PROJECT_DIR && $OXIDE serve --port $PORT" Enter
wait_for_server

say "Control plane running on port $PORT"
dim "$(curl -sf "$BASE/health" | jq -c .)"
wait_key

# ── Act 2: Register Devices & Fleet ──────────────────────────

step "2 · REGISTER DEVICES + CREATE FLEET"

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

say "Fleet 'factory' created with 3 devices"
echo ""
curl -sf "$BASE/api/v1/devices" | jq -r '.[] | "    \(.id)  \(.name)  [\(.status)]"'
wait_key

# ── Act 3: Upload Initial Model ──────────────────────────────

step "3 · UPLOAD MODEL"

V1_SIZE=$(wc -c < "$MODEL_V1" | tr -d ' ')
say "Uploading digit-detect@v1.0.0 (MLP-MNIST, 535K params, ${V1_SIZE} bytes)"
RESP=$(curl -sf -X POST "$BASE/api/v1/models/digit-detect/versions/v1.0.0" \
    --data-binary @"$MODEL_V1")
dim "sha256: $(echo "$RESP" | jq -r '.sha256[0:16]')..."
dim "stored in control plane model store"
wait_key

# ── Act 4: Create Campaign & Start Agents ────────────────────

step "4 · CREATE DEPLOYMENT CAMPAIGN"

say "Creating campaign to roll out v1.0.0 to the factory fleet..."
CAMP=$(curl -sf -X POST "$BASE/api/v1/campaigns" \
    -H "Content-Type: application/json" \
    -d '{"model_id": "digit-detect", "model_version": "v1.0.0", "fleet_id": "factory"}')
CAMP_ID=$(echo "$CAMP" | jq -r .campaign_id)
say "Campaign: $CAMP_ID"
dim "$(echo "$CAMP" | jq -c .)"
echo ""

say "Starting 3 agents — watch the bottom panes"
echo ""

for i in 1 2 3; do
    IDX=$((i - 1))
    tmux send-keys -t "${AGENTS[$IDX]}" \
        "$OXIDE agent --control-plane $BASE --device-id edge-$i --poll-interval 5 --model-dir /tmp/oxide-demo-agent-$i" Enter
    sleep 0.3
done

say "Waiting for all agents to download and apply v1.0.0..."
echo ""

for i in 1 2 3; do
    wait_for_agent "/tmp/oxide-demo-agent-$i" "v1.0.0"
    say "  edge-$i: v1.0.0 applied"
done
wait_key

# ── Act 5: Check Campaign ────────────────────────────────────

step "5 · CAMPAIGN STATUS"

say "Checking campaign progress..."
# Give heartbeats a moment to report
sleep 6

CAMP_STATUS=$(curl -sf "$BASE/api/v1/campaigns/$CAMP_ID")
echo "$CAMP_STATUS" | jq '{
  state: .state,
  total: .summary.total,
  complete: .summary.complete,
  failed: .summary.failed
}'
say "All devices updated via campaign"
wait_key

# ── Act 6: Upload Fine-Tuned Model ───────────────────────────

step "6 · UPLOAD FINE-TUNED MODEL (LAST LAYER ONLY)"

say "A data scientist retrained the last layer."
say "The model file is the same size — only w3 and b3 changed."
echo ""

V2_SIZE=$(wc -c < "$MODEL_V2" | tr -d ' ')
say "Uploading digit-detect@v2.0.0 (${V2_SIZE} bytes)..."
RESP=$(curl -sf -X POST "$BASE/api/v1/models/digit-detect/versions/v2.0.0" \
    --data-binary @"$MODEL_V2")

echo ""
say "The control plane automatically computed a delta:"
dim "Full model:  ${V2_SIZE} bytes"

# Check the delta file
DELTA_FILE=$(find "$PROJECT_DIR/.oxide/models/digit-detect/deltas" -name "*.oxdl" 2>/dev/null | head -1)
if [[ -n "$DELTA_FILE" ]]; then
    DELTA_SIZE=$(wc -c < "$DELTA_FILE" | tr -d ' ')
    SAVINGS=$(python3 -c "print(f'{(1 - $DELTA_SIZE / $V2_SIZE) * 100:.1f}')")
    hi "Delta patch: ${DELTA_SIZE} bytes (${SAVINGS}% bandwidth saved)"
else
    dim "No delta found (different model architecture?)"
fi
wait_key

# ── Act 7: OTA Update via Delta ──────────────────────────────

step "7 · FLEET OTA UPDATE — DELTA DELIVERY"

say "Creating campaign for v2.0.0..."
CAMP2=$(curl -sf -X POST "$BASE/api/v1/campaigns" \
    -H "Content-Type: application/json" \
    -d '{"model_id": "digit-detect", "model_version": "v2.0.0", "fleet_id": "factory"}')
CAMP2_ID=$(echo "$CAMP2" | jq -r .campaign_id)
say "Campaign: $CAMP2_ID"
echo ""

say "Agents will receive the delta patch, not the full file."
say "Watch the bottom panes — look for 'received delta' ↓"
echo ""

for i in 1 2 3; do
    wait_for_agent "/tmp/oxide-demo-agent-$i" "v2.0.0"
    say "  edge-$i: v2.0.0 applied via delta"
done
wait_key

# ── Act 8: Final State ───────────────────────────────────────

step "8 · FINAL STATE"

say "All 3 devices updated."
echo ""
for i in 1 2 3; do
    STATE=$(cat "/tmp/oxide-demo-agent-$i/.agent-state.json")
    say "  edge-$i: $(echo "$STATE" | jq -r '"\(.current_model)@\(.current_model_version)"')"
done

echo ""
say "Model versions in control plane:"
curl -sf "$BASE/api/v1/models/digit-detect" | \
    jq -r '.versions[] | "    \(.version)  \(.size_bytes) bytes  sha256:\(.sha256[0:12])..."'

echo ""
say "Agent disk (edge-1):"
find /tmp/oxide-demo-agent-1 -type f -not -name '.DS_Store' | sort | while read -r f; do
    SIZE=$(wc -c < "$f" | tr -d ' ')
    dim "$(echo "$f" | sed 's|/tmp/oxide-demo-agent-1/||')  ($SIZE bytes)"
done

# ── Summary ───────────────────────────────────────────────────

step "SUMMARY"

echo ""
say "What just happened:"
say "  1. Control plane stored two model versions"
say "  2. On upload, it computed a tensor-level delta (OXDL format)"
say "  3. Campaign tracked per-device rollout progress"
say "  4. Agents pulled v1 as a full download (${V1_SIZE} bytes)"
if [[ -n "${DELTA_SIZE:-}" ]]; then
say "  5. Agents pulled v2 as a delta patch (${DELTA_SIZE} bytes — ${SAVINGS}% saved)"
fi
say "  6. Each agent: download → verify SHA-256 → backup → apply → health check"
say "  7. On failure, automatic rollback to previous version"
echo ""
if [[ -n "${DELTA_SIZE:-}" ]]; then
hi "Bandwidth: 3 devices × ${DELTA_SIZE} bytes = $((DELTA_SIZE * 3)) bytes"
hi "Without delta: 3 × ${V2_SIZE} = $((V2_SIZE * 3)) bytes"
hi "Savings: ${SAVINGS}%"
fi
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
