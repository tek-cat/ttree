Output demo.mp4
Output demo.gif

Set FontSize 14
Set Width 1200
Set Height 674
Set Theme "Catppuccin Mocha"
Set Padding 0
Set TypingSpeed 80ms
Set PlaybackSpeed 1.0
Set Shell "bash"

# Sessions are pre-created by record.sh in /tmp/ttree-demo-socket
# TMUX_TMPDIR is passed to VHS via env, so all tmux calls find them
# No tmux nesting — ttree runs directly in VHS's own terminal

Hide
Type "clear"
Enter
Sleep 200ms
Show

# ── Launch ttree ──────────────────────────────────────────────────────────────
Sleep 500ms
Type "ttree"
Enter
Sleep 3500ms

# ── Navigate the tree ─────────────────────────────────────────────────────────
Down
Sleep 800ms
Down
Sleep 800ms
Down
Sleep 800ms
Up
Sleep 600ms
Up
Sleep 600ms

# ── Expand the agents session (3 windows) ─────────────────────────────────────
Down
Sleep 500ms
Down
Sleep 500ms
Space
Sleep 1200ms
Down
Sleep 700ms
Down
Sleep 700ms
Down
Sleep 700ms
Space
Sleep 1200ms

# ── Open live preview ─────────────────────────────────────────────────────────
Up
Sleep 500ms
Up
Sleep 500ms
Up
Sleep 500ms
Enter
Sleep 3000ms

# ── Switch sessions in preview via sidebar ────────────────────────────────────
Down
Sleep 1500ms
Down
Sleep 1500ms
Down
Sleep 1500ms

# ── Back to tree ─────────────────────────────────────────────────────────────
Ctrl+p
Sleep 1000ms

# ── New session ───────────────────────────────────────────────────────────────
Type "n"
Sleep 800ms
Type "new-feature"
Sleep 500ms
Enter
Sleep 1500ms

# ── Rename ────────────────────────────────────────────────────────────────────
Up
Sleep 500ms
Up
Sleep 500ms
Type "r"
Sleep 800ms
Backspace
Backspace
Backspace
Backspace
Backspace
Type "release"
Sleep 500ms
Enter
Sleep 1500ms

# ── Quit ──────────────────────────────────────────────────────────────────────
Type "q"
Sleep 1000ms
