#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

cleanup() {
    echo "==> Cleaning up demo sessions..."
    tmux kill-session -t work       2>/dev/null || true
    tmux kill-session -t monitoring 2>/dev/null || true
    tmux kill-session -t build      2>/dev/null || true
    tmux kill-session -t agents     2>/dev/null || true
}
trap cleanup EXIT

echo "==> Creating demo tmux sessions..."
# Kill any leftover demo sessions from previous runs
for s in work monitoring build agents; do tmux kill-session -t $s 2>/dev/null || true; done

tmux new-session -d -s work       -x 200 -y 50 '/bin/bash --norc --noprofile'
tmux send-keys   -t work 'vim ~/Projects/ttree/src/main.rs' Enter
sleep 0.5

tmux new-session -d -s monitoring -x 200 -y 50 '/bin/bash --norc --noprofile'
tmux send-keys   -t monitoring 'htop' Enter
sleep 0.5

tmux new-session -d -s build      -x 200 -y 50 '/bin/bash --norc --noprofile'
tmux send-keys   -t build 'echo build complete' Enter
sleep 0.2

tmux new-session -d -s agents     -x 200 -y 50 '/bin/bash --norc --noprofile'
tmux new-window  -t agents
tmux new-window  -t agents
tmux send-keys   -t agents:0 'echo [agent-1] running analysis'    Enter
tmux send-keys   -t agents:1 'echo [agent-2] waiting for results' Enter
tmux send-keys   -t agents:2 'echo [agent-3] synthesizing output' Enter
sleep 0.3

echo "==> Building ttree..."
cd ..
cargo build --release
cd demo

echo "==> Recording with VHS..."
# Unset TMUX so ttree runs as if outside any tmux session
unset TMUX
unset TMUX_PANE
vhs demo.tape

echo ""
echo "Done!"
ls -lh demo.mp4 demo.gif 2>/dev/null
