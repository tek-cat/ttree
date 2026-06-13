#!/usr/bin/env bash
# Capture README screenshots via VHS.
# Spins up an isolated tmux server (via TMUX_TMPDIR) so ttree shows only the
# demo sessions, not the user's real ones.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

export TMUX_TMPDIR="$(mktemp -d -t ttree-screenshots-XXXXXX)"

cleanup() {
    echo "==> Cleaning up demo sessions..."
    tmux kill-server 2>/dev/null || true
    rm -rf "$TMUX_TMPDIR"
}
trap cleanup EXIT

echo "==> Creating demo tmux sessions in $TMUX_TMPDIR ..."

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

mkdir -p ../docs/screenshots

echo "==> Capturing screenshots with VHS..."
# Unset TMUX so ttree runs as if outside any tmux session inside VHS.
unset TMUX
unset TMUX_PANE
vhs screenshots.tape

# VHS requires an Output directive; discard the dummy video.
rm -f screenshots-dummy.mp4 screenshots-dummy.gif

echo ""
echo "Done!"
ls -lh ../docs/screenshots/*.png 2>/dev/null
