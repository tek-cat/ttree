Output demo.mp4
Output demo.gif

Set FontSize 14
Set Width 1200
Set Height 674
Set Theme "Catppuccin Mocha"
Set Padding 0
Set TypingSpeed 80ms

# ── Hidden setup: isolated demo environment on ttree-demo socket ──────────────
Hide

Type "tmux -L ttree-demo kill-server 2>/dev/null; true"
Enter
Sleep 1s

Type "tmux -L ttree-demo new-session -d -s work -x 140 -y 33"
Enter
Sleep 100ms
Type "tmux -L ttree-demo send-keys -t work 'vim src/main.rs' Enter"
Enter
Sleep 200ms

Type "tmux -L ttree-demo new-session -d -s monitoring -x 140 -y 33"
Enter
Sleep 100ms
Type "tmux -L ttree-demo send-keys -t monitoring 'htop' Enter"
Enter
Sleep 200ms

Type "tmux -L ttree-demo new-session -d -s build -x 140 -y 33"
Enter
Sleep 100ms
Type "tmux -L ttree-demo send-keys -t build 'cargo build --release' Enter"
Enter
Sleep 300ms

Type "tmux -L ttree-demo new-session -d -s agents -x 140 -y 33"
Enter
Type "tmux -L ttree-demo new-window -t agents"
Enter
Type "tmux -L ttree-demo new-window -t agents"
Enter
Type "tmux -L ttree-demo send-keys -t agents:0 'echo [agent-1] running' Enter"
Enter
Type "tmux -L ttree-demo send-keys -t agents:1 'echo [agent-2] processing' Enter"
Enter
Type "tmux -L ttree-demo send-keys -t agents:2 'echo [agent-3] idle' Enter"
Enter
Sleep 300ms

Type "tmux -L ttree-demo new-session -d -s ttree -x 140 -y 37"
Enter
Sleep 100ms
Type "tmux -L ttree-demo split-window -v -l 4 -t ttree:0"
Enter
Sleep 100ms
Type "tmux -L ttree-demo send-keys -t ttree:0.1 'cd ~/Projects/ttree/demo && clear' Enter"
Enter
Sleep 200ms

Type "tmux -L ttree-demo attach-session -t ttree"
Enter
Sleep 1500ms

Show

# ── Startup description ───────────────────────────────────────────────────────
Ctrl+b
Type "o"
Sleep 200ms
Set TypingSpeed 0ms
Type './desc.sh "ttree" "a live tmux session manager — tree navigation with embedded pane preview"'
Enter
Set TypingSpeed 80ms
Sleep 200ms
Ctrl+b
Type "o"
Sleep 500ms

# ══════════════════════════════════════════════════════════════════════════════
# SECTION 1 — Launch
# ══════════════════════════════════════════════════════════════════════════════

Type "ttree"
Enter
Sleep 3500ms

# ══════════════════════════════════════════════════════════════════════════════
# SECTION 2 — Tree navigation
# ══════════════════════════════════════════════════════════════════════════════

Ctrl+b
Type "o"
Sleep 300ms
Set TypingSpeed 0ms
Type './desc.sh "Tree navigation" "j / k  to move up and down  ·  Space to expand or collapse"'
Enter
Set TypingSpeed 80ms
Sleep 300ms
Ctrl+b
Type "o"
Sleep 800ms

Down
Sleep 700ms
Down
Sleep 700ms
Down
Sleep 700ms
Up
Sleep 700ms
Up
Sleep 700ms

# ══════════════════════════════════════════════════════════════════════════════
# SECTION 3 — Expand / collapse hierarchy
# ══════════════════════════════════════════════════════════════════════════════

Ctrl+b
Type "o"
Sleep 300ms
Set TypingSpeed 0ms
Type './desc.sh "Expand / collapse" "the agents session has 3 windows — press Space to reveal them"'
Enter
Set TypingSpeed 80ms
Sleep 300ms
Ctrl+b
Type "o"
Sleep 800ms

Down
Sleep 600ms
Down
Sleep 600ms
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

# ══════════════════════════════════════════════════════════════════════════════
# SECTION 4 — Live preview
# ══════════════════════════════════════════════════════════════════════════════

Ctrl+b
Type "o"
Sleep 300ms
Set TypingSpeed 0ms
Type './desc.sh "Live pane preview" "press Enter to open an embedded terminal showing the active pane in real time"'
Enter
Set TypingSpeed 80ms
Sleep 300ms
Ctrl+b
Type "o"
Sleep 800ms

Up
Sleep 500ms
Up
Sleep 500ms
Enter
Sleep 3000ms

# ══════════════════════════════════════════════════════════════════════════════
# SECTION 5 — Switch sessions while staying in preview
# ══════════════════════════════════════════════════════════════════════════════

Ctrl+b
Type "o"
Sleep 300ms
Set TypingSpeed 0ms
Type './desc.sh "Session switching" "click or navigate the sidebar to switch sessions — preview updates instantly"'
Enter
Set TypingSpeed 80ms
Sleep 300ms
Ctrl+b
Type "o"
Sleep 800ms

Down
Sleep 1500ms
Down
Sleep 1500ms
Down
Sleep 1500ms

Ctrl+p
Sleep 1000ms

# ══════════════════════════════════════════════════════════════════════════════
# SECTION 6 — New session
# ══════════════════════════════════════════════════════════════════════════════

Ctrl+b
Type "o"
Sleep 300ms
Set TypingSpeed 0ms
Type './desc.sh "New session" "press n to create a session — it appears in the tree immediately"'
Enter
Set TypingSpeed 80ms
Sleep 300ms
Ctrl+b
Type "o"
Sleep 800ms

Type "n"
Sleep 800ms
Type "new-feature"
Sleep 600ms
Enter
Sleep 2000ms

# ══════════════════════════════════════════════════════════════════════════════
# SECTION 7 — Rename
# ══════════════════════════════════════════════════════════════════════════════

Ctrl+b
Type "o"
Sleep 300ms
Set TypingSpeed 0ms
Type './desc.sh "Rename" "press r to rename the selected session inline"'
Enter
Set TypingSpeed 80ms
Sleep 300ms
Ctrl+b
Type "o"
Sleep 800ms

Up
Sleep 600ms
Up
Sleep 600ms
Type "r"
Sleep 800ms
Backspace
Backspace
Backspace
Backspace
Backspace
Type "release"
Sleep 600ms
Enter
Sleep 1800ms

# ══════════════════════════════════════════════════════════════════════════════
# SECTION 8 — Attach
# ══════════════════════════════════════════════════════════════════════════════

Ctrl+b
Type "o"
Sleep 300ms
Set TypingSpeed 0ms
Type './desc.sh "Attach" "press a to detach ttree and attach directly to the selected session"'
Enter
Set TypingSpeed 80ms
Sleep 300ms
Ctrl+b
Type "o"
Sleep 800ms

Down
Sleep 600ms
Type "a"
Sleep 3000ms
