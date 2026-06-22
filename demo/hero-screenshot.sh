#!/usr/bin/env bash
# Capture the README hero screenshot -> docs/screenshot.png (via VHS).
#
# The shot: ttree focused on a 2-pane session (vim on src/main.rs above an
# interactive git log), with background sessions (vps/watch/web) in the sidebar
# tree. Fewer panes + a larger font so the terminal text stays legible (at least
# as big as the tagline) once the image is scaled down on the landing page.
#
# Everything runs against a THROWAWAY tmux server so your real sessions are
# never touched. Safety invariants:
#   1. unset TMUX/TMUX_PANE before ANY tmux call, so TMUX_TMPDIR is honored.
#      (If run from inside tmux, $TMUX would otherwise override TMUX_TMPDIR and
#       every tmux command -- including the cleanup kill-server -- would hit
#       your REAL server.)
#   2. a throwaway TMUX_TMPDIR socket; nothing here can reach your default server.
#   3. a uniquely-named probe verifies the socket lives under TMUX_TMPDIR BEFORE
#      any real-looking session is created; abort otherwise, killing only the
#      unique probe -- never a bare name that could match a real session.
#   4. the cleanup trap runs kill-server only when the live socket is proven ours.
#   5. a throwaway XDG_CONFIG_HOME, so ~/.config/ttree/state.toml is never touched.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"

unset TMUX TMUX_PANE
export TMUX_TMPDIR="$(mktemp -d -t ttree-hero-XXXXXX)"
mkdir -p "$TMUX_TMPDIR/bin" "$TMUX_TMPDIR/xdg/ttree"
export XDG_CONFIG_HOME="$TMUX_TMPDIR/xdg"
export PATH="$TMUX_TMPDIR/bin:$REPO/target/release:$PATH"

# Seeded ttree state: narrow sidebar + empty expanded_ids (-> auto-expand-all on
# first load, matching a fresh-config look). Lives in the throwaway config only.
cat > "$XDG_CONFIG_HOME/ttree/state.toml" <<'CFG'
expanded_ids = []
sidebar_cols = 24

[focus]
panel = "Tree"
nav_mode = "Window"
scroll_offset = 0
enable_scrolling = false
CFG

guarded_cleanup(){
  local sp; sp="$(tmux display-message -p '#{socket_path}' 2>/dev/null || true)"
  if [[ "$TMUX_TMPDIR" == */ttree-hero-* && ( -z "$sp" || "$sp" == "$TMUX_TMPDIR"* ) ]]; then
    tmux kill-server 2>/dev/null || true
  fi
  rm -rf "$TMUX_TMPDIR"
}
trap guarded_cleanup EXIT

echo "==> Building ttree (release)..."
( cd "$REPO" && cargo build --release )

# ---- isolation gate (unique probe name; cannot collide with real sessions) ----
PROBE="ttreehero_probe_$$"
tmux new-session -d -s "$PROBE" -x 250 -y 62 'bash --norc --noprofile'
SP="$(tmux display-message -t "$PROBE" -p '#{socket_path}')"
case "$SP" in
  "$TMUX_TMPDIR"*) echo "ISOLATION_OK ($SP)" ;;
  *) echo "!! NOT isolated (socket: $SP); aborting to protect real sessions." >&2
     tmux kill-session -t "$PROBE" 2>/dev/null || true; trap - EXIT; rm -rf "$TMUX_TMPDIR"; exit 3 ;;
esac
tmux kill-session -t "$PROBE"
# Killing the server's only session shuts it down; wait for that before the next
# new-session, which would otherwise race the dying server ("server exited
# unexpectedly").
sleep 0.3

echo "==> Creating demo sessions in $TMUX_TMPDIR ..."
# Focused session: ttree, one window, 2 panes stacked vertically (vim on
# src/main.rs above an interactive git log). Stacking keeps each pane full width,
# so both stay legible at the larger font.
GITLOG="git -c color.ui=always log --graph --oneline --decorate --all | less -R"
tmux new-session -d -s ttree -x 120 -y 40 'bash --norc --noprofile'
VIM_PANE="$(tmux display-message -t ttree -p '#{pane_id}')"
tmux send-keys -t "$VIM_PANE" "cd '$REPO' && clear && vim src/main.rs" Enter; sleep 0.8
GIT_PANE="$(tmux split-window -v -t "$VIM_PANE" -l 40% -P -F '#{pane_id}' 'bash --norc --noprofile')"
tmux send-keys -t "$GIT_PANE" "cd '$REPO' && clear && $GITLOG" Enter; sleep 0.5

# Background sessions, so the tree has siblings.
tmux new-session -d -s vps   -x 200 -y 50 'bash --norc --noprofile'; tmux send-keys -t vps 'htop' Enter
tmux new-window  -t vps 'bash --norc --noprofile';                   tmux send-keys -t vps 'journalctl -f' Enter
tmux new-session -d -s watch -x 200 -y 50 'bash --norc --noprofile'; tmux send-keys -t watch 'cargo watch -x build' Enter
tmux new-session -d -s web   -x 200 -y 50 'bash --norc --noprofile'; tmux send-keys -t web 'python3 -m http.server 8088' Enter
sleep 0.3
tmux select-pane -t "$VIM_PANE"

echo "==> Capturing screenshot with VHS..."
( cd "$SCRIPT_DIR" && vhs hero-screenshot.tape )
rm -f "$SCRIPT_DIR/hero-dummy.mp4"   # VHS requires an Output directive; discard the dummy.

echo "Done -> docs/screenshot.png"
