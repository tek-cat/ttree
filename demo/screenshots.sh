#!/usr/bin/env bash
# Capture README screenshots via VHS.
# Spins up an isolated tmux server (via TMUX_TMPDIR) so ttree shows only the
# demo sessions, not the user's real ones.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# CRITICAL: drop any inherited tmux context BEFORE the first tmux call.
# If this script runs from inside a tmux session, $TMUX names the real
# server's socket and overrides TMUX_TMPDIR -- every "tmux new-session"
# below would then land on your REAL server, and the cleanup trap's
# "tmux kill-server" would kill ALL your real sessions. Unsetting first
# makes every tmux command honor TMUX_TMPDIR (its own throwaway socket).
unset TMUX TMUX_PANE
export TMUX_TMPDIR="$(mktemp -d -t ttree-screenshots-XXXXXX)"

cleanup() {
    echo "==> Cleaning up demo sessions..."
    # Guard: only ever kill a server whose socket lives under OUR temp dir.
    local sp; sp="$(tmux display-message -p '#{socket_path}' 2>/dev/null || true)"
    if [[ "$TMUX_TMPDIR" == */ttree-screenshots-* && ( -z "$sp" || "$sp" == "$TMUX_TMPDIR"* ) ]]; then
        tmux kill-server 2>/dev/null || true
    fi
    rm -rf "$TMUX_TMPDIR"
}
trap cleanup EXIT

echo "==> Creating demo tmux sessions in $TMUX_TMPDIR ..."

# Hard isolation gate: create one probe session and verify its socket lives
# under TMUX_TMPDIR before doing anything else. If not, abort without ever
# running kill-server against whatever server we accidentally hit.
tmux new-session -d -s work -x 200 -y 50 '/bin/bash --norc --noprofile'
SOCK_PATH="$(tmux display-message -t work -p '#{socket_path}')"
case "$SOCK_PATH" in
    "$TMUX_TMPDIR"*) : ;;  # isolated, good
    *)
        echo "!! tmux is NOT isolated (socket: $SOCK_PATH); aborting to protect real sessions." >&2
        tmux kill-session -t work 2>/dev/null || true
        trap - EXIT
        rm -rf "$TMUX_TMPDIR"
        exit 3
        ;;
esac

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
vhs screenshots.tape

# VHS requires an Output directive; discard the dummy video.
rm -f screenshots-dummy.mp4 screenshots-dummy.gif

echo ""
echo "Done!"
ls -lh ../docs/screenshots/*.png 2>/dev/null
