Output screenshots-dummy.mp4

Set FontSize 14
Set Width 1200
Set Height 674
Set Theme "Catppuccin Mocha"
Set Padding 0
Set TypingSpeed 50ms
Set Shell "bash"

# Sessions are pre-created by screenshots.sh in an isolated tmux socket
# (TMUX_TMPDIR env var). ttree picks them up via the tmux CLI.

Hide
Type "clear"
Enter
Sleep 200ms
Show

# ── Launch ttree ──────────────────────────────────────────────────────────────
Sleep 400ms
Type "ttree"
Enter
Sleep 2500ms

# ── State 1: tree expanded ────────────────────────────────────────────────────
# Default selection lands on the first session ("agents" alphabetically).
# Expand agents to reveal its 3 windows, then move selection onto one.
Space
Sleep 600ms
Down
Sleep 300ms
Down
Sleep 300ms
Screenshot ../docs/screenshots/tree-overview.png

# ── State 2: live preview on the work session ────────────────────────────────
# Move down past the agents windows to the "work" session, open preview.
Down
Sleep 200ms
Down
Sleep 200ms
Down
Sleep 200ms
Down
Sleep 200ms
Down
Sleep 200ms
Enter
Sleep 2500ms
Screenshot ../docs/screenshots/live-preview.png

# ── Quit ──────────────────────────────────────────────────────────────────────
Ctrl+p
Sleep 300ms
Type "q"
Sleep 500ms
