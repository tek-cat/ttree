#!/usr/bin/env bash
# Capture the README hero screenshot -> docs/screenshot.png (via VHS).
#
# The shot: ttree focused on a 3-pane session (vim on src/main.rs, an
# interactive git log on the right, a debug pane lower-left), with background
# sessions (vps/watch/web) in the sidebar tree.
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
sidebar_cols = 29

[focus]
panel = "Tree"
nav_mode = "Window"
scroll_offset = 0
enable_scrolling = false
CFG

# Lower-left "debug" pane content.
cat > "$TMUX_TMPDIR/bin/ttree-debug" <<'DBG'
#!/usr/bin/env bash
tmux list-sessions -F '  #{session_name} (#{session_windows}w)'
echo '  --- focused window ---'
tmux list-panes -t ttree -F '  ttree.#{pane_index} -> #{pane_current_command}'
echo '[debug] tmux sync 4ms · interval 200ms · preview %0'
DBG
chmod +x "$TMUX_TMPDIR/bin/ttree-debug"

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

echo "==> Creating demo sessions in $TMUX_TMPDIR ..."
# Focused session: ttree, one window, 3 panes (vim left, git log right @45%, debug lower-left).
GITLOG="git -c color.ui=always log --graph --oneline --decorate --all | less -R"
tmux new-session -d -s ttree -x 250 -y 62 'bash --norc --noprofile'
VIM_PANE="$(tmux display-message -t ttree -p '#{pane_id}')"
tmux send-keys -t "$VIM_PANE" "cd '$REPO' && clear && vim src/main.rs" Enter; sleep 0.8
GIT_PANE="$(tmux split-window -h -t ttree -l 45% -P -F '#{pane_id}' 'bash --norc --noprofile')"
tmux send-keys -t "$GIT_PANE" "cd '$REPO' && clear && $GITLOG" Enter; sleep 0.5
DBG_PANE="$(tmux split-window -v -t "$VIM_PANE" -l 34% -P -F '#{pane_id}' 'bash --norc --noprofile')"

# Background sessions, so the tree has siblings.
tmux new-session -d -s vps   -x 200 -y 50 'bash --norc --noprofile'; tmux send-keys -t vps 'htop' Enter
tmux new-window  -t vps 'bash --norc --noprofile';                   tmux send-keys -t vps 'journalctl -f' Enter
tmux new-session -d -s watch -x 200 -y 50 'bash --norc --noprofile'; tmux send-keys -t watch 'cargo watch -x build' Enter
tmux new-session -d -s web   -x 200 -y 50 'bash --norc --noprofile'; tmux send-keys -t web 'python3 -m http.server 8088' Enter
sleep 0.3
tmux send-keys -t "$DBG_PANE" 'clear && ttree-debug' Enter
sleep 0.3
tmux select-pane -t "$VIM_PANE"

echo "==> Capturing screenshot with VHS..."
( cd "$SCRIPT_DIR" && vhs hero-screenshot.tape )
rm -f "$SCRIPT_DIR/hero-dummy.mp4"   # VHS requires an Output directive; discard the dummy.

echo "Done -> docs/screenshot.png"
