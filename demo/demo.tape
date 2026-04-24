Output demo.mp4
Output demo.gif

Set FontSize 14
Set Width 1200
Set Height 675
Set Theme "Catppuccin Mocha"
Set Padding 0
Set TypingSpeed 80ms
Set PlaybackSpeed 1.0

# ── Hidden setup: build demo tmux environment ─────────────────────────────────
Hide

# Clean slate
Type "tmux kill-server 2>/dev/null; true"
Enter
Sleep 800ms

# Session: work  (vim editing a Rust file)
Type "tmux new-session -d -s work -x 200 -y 50"
Enter
Type "tmux send-keys -t work 'vim src/main.rs' Enter"
Enter
Sleep 300ms

# Session: monitoring  (htop)
Type "tmux new-session -d -s monitoring -x 200 -y 50"
Enter
Type "tmux send-keys -t monitoring 'htop' Enter"
Enter
Sleep 300ms

# Session: build  (idle shell, simulate a finished build)
Type "tmux new-session -d -s build -x 200 -y 50"
Enter
Type "tmux send-keys -t build 'cargo build --release 2>&1 | tail -3' Enter"
Enter
Sleep 300ms

# Session: agents  (multiple windows to show hierarchy)
Type "tmux new-session -d -s agents -x 200 -y 50"
Enter
Type "tmux new-window -t agents"
Enter
Type "tmux new-window -t agents"
Enter
Type "tmux send-keys -t agents:0 'echo Running agent task...' Enter"
Enter
Type "tmux send-keys -t agents:1 'echo Waiting for results...' Enter"
Enter
Type "tmux send-keys -t agents:2 'echo Processing output...' Enter"
Enter
Sleep 500ms

Show

# ── Launch ttree ──────────────────────────────────────────────────────────────
Type "ttree"
Enter
Sleep 2500ms

# ── Navigate the tree ─────────────────────────────────────────────────────────
# Move down through sessions
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms

# Expand the agents session (it has multiple windows)
Space
Sleep 600ms
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms

# Collapse it back
Space
Sleep 600ms

# Navigate back up to monitoring
Up
Sleep 350ms
Up
Sleep 350ms

# ── Open preview panel ────────────────────────────────────────────────────────
Sleep 500ms
Enter
Sleep 2000ms

# ── Switch sessions in preview by navigating sidebar ─────────────────────────
Down
Sleep 800ms
Down
Sleep 800ms
Down
Sleep 800ms

# ── Return to tree mode ───────────────────────────────────────────────────────
Ctrl+p
Sleep 600ms

# ── Create a new session ──────────────────────────────────────────────────────
Type "n"
Sleep 600ms
Type "new-feature"
Sleep 400ms
Enter
Sleep 1200ms

# ── Rename a session ─────────────────────────────────────────────────────────
# Navigate to the build session
Up
Sleep 350ms
Up
Sleep 350ms
Type "r"
Sleep 600ms
# Clear pre-filled name and type new one
Backspace
Backspace
Backspace
Backspace
Backspace
Type "release"
Sleep 400ms
Enter
Sleep 1000ms

# ── Attach to a session ───────────────────────────────────────────────────────
Type "a"
Sleep 2000ms
