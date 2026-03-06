#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# Oxide Fleet OTA Demo
#
# Spins up a tmux session with 5 panes:
#   ┌─────────────────────┬────────────────────┐
#   │   🎬 narrator       │  ⚡ control-plane   │
#   ├───────────┬─────────┴──┬─────────────────┤
#   │ agent-01  │  agent-02  │    agent-03      │
#   └───────────┴────────────┴─────────────────┘
#
# Usage:  ./scripts/demo.sh
# Prereqs: tmux, curl, jq, release binary
# ──────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
OXIDE="$PROJECT_DIR/target/release/oxide"
SESSION="oxide-demo"

for cmd in tmux curl jq; do
    command -v "$cmd" &>/dev/null || { echo "Missing: $cmd"; exit 1; }
done
[[ -x "$OXIDE" ]] || { echo "Build first: cargo build --release -p oxide-cli"; exit 1; }

# Clean slate
tmux kill-session -t "$SESSION" 2>/dev/null || true
rm -rf /tmp/oxide-demo-* "$PROJECT_DIR/.oxide"
for i in 1 2 3; do mkdir -p "/tmp/oxide-demo-agent-$i"; done

# ── Build layout using pane IDs (stable across re-indexing) ───

tmux new-session -d -s "$SESSION" -x 200 -y 50

# Pane %A = full window. Split top/bottom.
tmux split-window -v -t "$SESSION" -l 20
# Now: top pane (1) = narrator area, bottom pane (new) = agents area

# Capture pane IDs
NARRATOR=$(tmux list-panes -t "$SESSION" -F '#{pane_id}' | head -1)
BOTTOM=$(tmux list-panes -t "$SESSION" -F '#{pane_id}' | tail -1)

# Split top into narrator (left) + control-plane (right)
tmux split-window -h -t "$NARRATOR" -p 50
CP_PANE=$(tmux list-panes -t "$SESSION" -F '#{pane_id}' | sed -n '2p')

# Split bottom into 3 agent panes
tmux split-window -h -t "$BOTTOM" -p 66
BOT_RIGHT=$(tmux list-panes -t "$SESSION" -F '#{pane_id}' | tail -1)
tmux split-window -h -t "$BOT_RIGHT" -p 50

# Collect final pane IDs in order
PANES=($(tmux list-panes -t "$SESSION" -F '#{pane_id}'))
# PANES: [0]=narrator, [1]=control-plane, [2]=agent-1, [3]=agent-2, [4]=agent-3

# Label panes
tmux select-pane -t "${PANES[0]}" -T "🎬 demo"
tmux select-pane -t "${PANES[1]}" -T "⚡ control-plane"
tmux select-pane -t "${PANES[2]}" -T "🤖 edge-1"
tmux select-pane -t "${PANES[3]}" -T "🤖 edge-2"
tmux select-pane -t "${PANES[4]}" -T "🤖 edge-3"

tmux set-option -t "$SESSION" pane-border-status top
tmux set-option -t "$SESSION" pane-border-format " #{pane_title} "

# Export pane IDs for the inner script
export DEMO_PANE_CP="${PANES[1]}"
export DEMO_PANE_A1="${PANES[2]}"
export DEMO_PANE_A2="${PANES[3]}"
export DEMO_PANE_A3="${PANES[4]}"

# Focus narrator and launch inner script
tmux select-pane -t "${PANES[0]}"
tmux send-keys -t "${PANES[0]}" \
    "export DEMO_PANE_CP=${PANES[1]} DEMO_PANE_A1=${PANES[2]} DEMO_PANE_A2=${PANES[3]} DEMO_PANE_A3=${PANES[4]}; bash $SCRIPT_DIR/_demo_inner.sh" Enter

tmux attach-session -t "$SESSION"
