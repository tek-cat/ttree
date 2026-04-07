# Tmux Control Mode (`-CC`) Session Manager — Technical Specification

**Project:** `mux` — A Rust/Ratatui tree-based tmux session manager  
**Architecture:** Control Mode `tmux -CC` relay with ratatui TUI shell  
**Version:** 0.1.0 (MVP Spec)

---

## Overview

`mux` is a terminal user interface for managing tmux sessions, windows, and panes using a tree-based layout. It communicates exclusively with a tmux server via **control mode** (`tmux -CC`), which provides structured event notifications and command responses over stdio without requiring a separate PTY per pane. For pane content rendering, raw VT byte streams from `%extended-output` notifications are relayed directly to the outer terminal using cursor-positioning escape sequences, bypassing any userspace VT100 parser and achieving near-native display performance.

---

## Architecture

### Process Model

```
┌─────────────────────────────────────────────────┐
│  mux process (Rust)                             │
│                                                 │
│  ┌────────────┐    ┌──────────────────────────┐ │
│  │ Ratatui    │    │ Control Mode Client      │ │
│  │ TUI Shell  │◄──►│ (tmux -CC stdio pipe)    │ │
│  │            │    │                          │ │
│  │ - Tree     │    │ - Command sender         │ │
│  │ - Status   │    │ - Event parser           │ │
│  │ - Keybinds │    │ - Output relay           │ │
│  └────────────┘    └──────────────────────────┘ │
│         │                      │                │
│         ▼                      ▼                │
│  ┌─────────────────────────────────────┐        │
│  │  AppState (tokio::sync::RwLock)     │        │
│  │  - SessionTree                      │        │
│  │  - FocusedPane                      │        │
│  │  - InputMode                        │        │
│  │  - PaneRegions (layout rects)       │        │
│  └─────────────────────────────────────┘        │
└─────────────────────────────────────────────────┘
         │
         ▼ stdout (raw relay + ratatui frames)
   Outer Terminal (Ghostty, Alacritty, etc.)
```

### Key Design Principle

The TUI shell (ratatui) and the pane content relay are **two separate write paths to stdout**:

1. **Ratatui path** — draws the chrome: tree sidebar, status bar, keybind hints, borders. Manages its own alternate screen buffer.
2. **Raw relay path** — when a pane preview or interactive pane is active, raw `%extended-output` bytes are written directly to stdout at the computed cursor region, with ratatui's rendering paused for that region.

This dual-path model means the outer terminal's native VT parser handles all pane content — zero userspace VT100 decoding.

---

## Control Mode Protocol

### Connecting to tmux

On startup, `mux` spawns tmux in control mode as a child process:

```rust
let mut child = tokio::process::Command::new("tmux")
    .args([
        "-CC",           // control mode, no canonical processing
        "new-session",   // or attach-session if server exists
        "-A",            // attach if session exists
        "-s", "main",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()?;
```

If a tmux server is already running, attach instead:

```rust
.args(["-CC", "attach-session", "-t", &session_name])
```

The `-CC` flag (double-C) disables canonical mode on the control channel, allowing `%extended-output` notifications with raw pane bytes[cite:49].

### Message Format

All control mode messages follow the structure:

```
%begin <timestamp> <command-id>
<response data>
%end <timestamp> <command-id>
```

Notification events (unsolicited) arrive outside `%begin/%end` blocks:

| Notification | Trigger | Key Fields |
|---|---|---|
| `%session-created` | New session | `$session-id` |
| `%session-closed` | Session exits | `$session-id` |
| `%window-add` | New window in session | `$session-id`, `@window-id` |
| `%window-close` | Window closed | `@window-id` |
| `%pane-exited` | Pane process exits | `%pane-id` |
| `%extended-output` | Pane produces output | `%pane-id`, raw bytes |
| `%layout-change` | Window layout resized | `@window-id`, layout string |
| `%client-detached` | Client detaches | client name |

### Parsing State Machine

The control mode reader runs on a dedicated `tokio::task::spawn_blocking` thread (non-async, tight read loop, zero allocation on hot path)[cite:56]:

```rust
enum ParseState {
    Idle,
    InBlock { cmd_id: u64, lines: Vec<String> },
}

fn parse_line(state: &mut ParseState, line: &str) -> Option<ControlEvent> {
    match (state, line) {
        (ParseState::Idle, l) if l.starts_with('%') => {
            // Parse notification: %session-created, %extended-output, etc.
            Some(parse_notification(l))
        }
        (ParseState::Idle, l) if l.starts_with("%begin") => {
            let cmd_id = parse_cmd_id(l);
            *state = ParseState::InBlock { cmd_id, lines: vec![] };
            None
        }
        (ParseState::InBlock { lines, .. }, l) if l.starts_with("%end") => {
            let block = std::mem::take(lines);
            *state = ParseState::Idle;
            Some(ControlEvent::Response(block))
        }
        (ParseState::InBlock { lines, .. }, l) => {
            lines.push(l.to_string());
            None
        }
        _ => None,
    }
}
```

---

## Raw Pane Output Relay

This is the core performance mechanism. When `%extended-output` arrives for a tracked pane, bytes are relayed directly to the outer terminal within a bounded cursor region — no VT100 parsing, no ratatui widget involvement[cite:49].

### Relay Sequence

```rust
async fn relay_pane_output(
    pane_id: &str,
    raw_bytes: &[u8],
    region: Rect,  // ratatui Rect — top/left/width/height in terminal cells
    stdout: &mut impl Write,
) -> io::Result<()> {
    // 1. Save outer cursor state
    stdout.write_all(b"\x1b7")?;                           // DECSC: save cursor

    // 2. Enable region scrolling within pane bounds
    stdout.write_all(
        format!("\x1b[{};{}r", region.top(), region.bottom()).as_bytes()
    )?;                                                     // DECSTBM: set scroll region

    // 3. Move cursor to top-left of pane region
    stdout.write_all(
        format!("\x1b[{};{}H", region.top() + 1, region.left() + 1).as_bytes()
    )?;                                                     // CUP: cursor position

    // 4. Write raw bytes — outer terminal parses natively
    stdout.write_all(raw_bytes)?;

    // 5. Reset scroll region and restore cursor
    stdout.write_all(b"\x1b[r")?;                          // reset scroll region
    stdout.write_all(b"\x1b8")?;                           // DECRC: restore cursor

    stdout.flush()?;
    Ok(())
}
```

### Flow Control

tmux control mode supports `%pause %pane-id` and `%continue %pane-id` commands to throttle pane output[cite:49]. Use these when the relay buffer exceeds a threshold to prevent output flooding:

```rust
const RELAY_BUFFER_LIMIT: usize = 64 * 1024; // 64KB

if relay_buffer.len() > RELAY_BUFFER_LIMIT {
    send_command(&mut stdin, &format!("%pause {}", pane_id)).await?;
    // Drain buffer, then resume
    send_command(&mut stdin, &format!("%continue {}", pane_id)).await?;
}
```

---

## Application State

```rust
#[derive(Debug, Clone)]
pub struct AppState {
    pub sessions: IndexMap<SessionId, Session>,
    pub windows: IndexMap<WindowId, Window>,
    pub panes: IndexMap<PaneId, Pane>,
    pub focus: Focus,
    pub input_mode: InputMode,
    pub layout: LayoutState,
    pub cmd_counter: u64,              // monotonic control mode command ID
    pub pending: HashMap<u64, Sender<Vec<String>>>,  // in-flight command responses
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub windows: Vec<WindowId>,
    pub created_at: u64,
    pub expanded: bool,                // tree expand/collapse state
}

#[derive(Debug, Clone)]
pub struct Pane {
    pub id: PaneId,
    pub window_id: WindowId,
    pub title: String,
    pub current_command: String,       // from `display-message -p '#{pane_current_command}'`
    pub current_path: String,
    pub active: bool,
    pub region: Option<Rect>,         // set when pane is rendered in relay mode
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    /// Ratatui handles all keypresses (navigation, commands)
    TuiNormal,
    /// TUI paused; raw bytes forwarded to focused pane via control mode `send-keys`
    PtyPassthrough { pane_id: PaneId },
    /// Fuzzy search overlay active
    FuzzySearch,
    /// Command input (`:` prefix, vim-style)
    Command,
}

#[derive(Debug, Clone)]
pub struct Focus {
    pub panel: Panel,                  // Tree | Preview | Pane
    pub session_idx: usize,
    pub window_idx: Option<usize>,
    pub pane_idx: Option<usize>,
}
```

---

## Layout

### Default Split (Three-Panel)

```
┌──────────────────────────────────────────────────────────────┐
│  mux  [normal]  session: main  3 sessions  12 windows        │  ← status bar
├──────────────────────────────────────────────────────────────┤
│ Sessions          │                                          │
│ ▼ main (3)        │   ╔══════════════════════════════════╗   │
│   ├─ ▶ editor    │   ║                                  ║   │
│   ├─   server    │   ║   PANE RELAY REGION              ║   │
│   └─   logs      │   ║   (raw %extended-output bytes)   ║   │
│ ▶ work (5)        │   ║                                  ║   │
│   ├─ ▶ api       │   ║                                  ║   │
│   └─   ...       │   ╚══════════════════════════════════╝   │
│ ▶ scratch (2)     │                                          │
│                   │   pane: %3  cmd: nvim  path: ~/proj      │
├──────────────────────────────────────────────────────────────┤
│  <Tab> focus  <Enter> attach  <n> new  <d> delete  <?> help  │  ← keybind bar
└──────────────────────────────────────────────────────────────┘
```

### Layout Calculation

Ratatui computes all `Rect` values. The pane relay region's `Rect` is stored in `AppState.panes[id].region` so the relay function knows exactly where to position cursor writes:

```rust
fn compute_layout(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let [status, body, keybinds] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ]).areas(area);

    let [tree, preview] = Layout::horizontal([
        Constraint::Percentage(28),
        Constraint::Fill(1),
    ]).areas(body);

    (status, tree, preview, keybinds)
}
```

The relay region is `preview` shrunk by 1 cell on each side (to account for the block border).

---

## Input Routing

```
Crossterm KeyEvent
        │
        ▼
┌───────────────────────┐
│  InputMode check      │
└───────────┬───────────┘
            │
     ┌──────┴──────┐
     │             │
TuiNormal    PtyPassthrough
     │             │
     ▼             ▼
Handle in    send-keys to tmux
ratatui      via control mode:
event loop   `send-keys -t %pane_id "KEY" Enter`
```

In `PtyPassthrough` mode, all key events (except the escape sequence `Ctrl-b Ctrl-b` which returns to `TuiNormal`) are serialized and sent to tmux via the control mode command:

```rust
fn key_event_to_tmux_key(event: KeyEvent) -> String {
    match event {
        KeyEvent { code: KeyCode::Enter, .. }     => "Enter".into(),
        KeyEvent { code: KeyCode::Backspace, .. } => "BSpace".into(),
        KeyEvent { code: KeyCode::Up, .. }        => "Up".into(),
        KeyEvent { code: KeyCode::Char(c), modifiers: KeyModifiers::CTRL, .. } => {
            format!("C-{}", c)
        }
        KeyEvent { code: KeyCode::Char(c), .. }   => c.to_string(),
        _ => String::new(),
    }
}

// Send to tmux
let cmd = format!("send-keys -t {} '{}' \n", pane_id, key_str);
control_stdin.write_all(cmd.as_bytes()).await?;
```

---

## Command Interface

All tmux mutations go through a typed command layer that writes to the control mode stdin and awaits a `%begin/%end` response block:

```rust
pub struct ControlClient {
    stdin: ChildStdin,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Vec<String>>>>>,
    counter: Arc<AtomicU64>,
}

impl ControlClient {
    pub async fn run_command(&self, cmd: &str) -> Result<Vec<String>> {
        let id = self.counter.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        // tmux ignores extra data before command — prefix with empty to get ID echo
        let line = format!("{}\n", cmd);
        self.stdin.write_all(line.as_bytes()).await?;
        Ok(rx.await?)
    }

    // Typed wrappers
    pub async fn new_session(&self, name: &str) -> Result<SessionId> { ... }
    pub async fn kill_session(&self, id: &SessionId) -> Result<()> { ... }
    pub async fn rename_session(&self, id: &SessionId, name: &str) -> Result<()> { ... }
    pub async fn new_window(&self, session: &SessionId, name: &str) -> Result<WindowId> { ... }
    pub async fn kill_window(&self, id: &WindowId) -> Result<()> { ... }
    pub async fn split_window(&self, pane: &PaneId, vertical: bool) -> Result<PaneId> { ... }
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> { ... }
    pub async fn subscribe_pane(&self, id: &PaneId) -> Result<()> {
        // Subscribe to %extended-output for this pane
        self.run_command(&format!("set -t {} monitor-activity on", id)).await?;
        Ok(())
    }
}
```

---

## Tree Widget

The session tree is a custom ratatui `StatefulWidget`. Each node is a `TreeNode` enum rendered as a `Line`:

```rust
pub enum TreeNode {
    Session { id: SessionId, name: String, window_count: usize, expanded: bool },
    Window  { id: WindowId,  name: String, pane_count: usize,   active: bool },
    Pane    { id: PaneId,    command: String, active: bool },
}

impl TreeNode {
    fn render_line(&self, focused: bool, depth: u8) -> Line<'static> {
        let indent = "  ".repeat(depth as usize);
        match self {
            TreeNode::Session { name, window_count, expanded, .. } => {
                let icon = if *expanded { "▼" } else { "▶" };
                let style = if focused { Style::new().bold().fg(PRIMARY) } else { Style::new() };
                Line::from(vec![
                    Span::raw(format!("{}{} ", indent, icon)),
                    Span::styled(name.clone(), style),
                    Span::styled(format!(" ({})", window_count), Style::new().fg(MUTED)),
                ])
            }
            // ... Window and Pane variants
        }
    }
}
```

Tree navigation uses vim-style keys (`j`/`k` to move, `l`/`h` or `Enter` to expand/collapse, `Space` to preview, `Enter` on a session to attach).

---

## Startup Sequence

```
1. Parse CLI args (session name, config path)
2. Check: is tmux server running? (`tmux ls` exit code)
   - No  → spawn new server + session via `tmux -CC new-session -s <name>`
   - Yes → `tmux -CC attach-session -A -s <name>`
3. Read initial state: run `list-sessions`, `list-windows -a`, `list-panes -a`
4. Populate AppState.sessions / windows / panes
5. Enter ratatui alternate screen
6. Subscribe to %extended-output for the currently focused pane
7. Start main event loop (tokio::select! over control_rx + crossterm events)
```

---

## Crate Dependencies

```toml
[dependencies]
ratatui        = "0.29"
crossterm      = "0.28"
tokio          = { version = "1", features = ["full"] }
indexmap       = "2"               # ordered session/window/pane maps
anyhow         = "1"
clap           = { version = "4", features = ["derive"] }
serde          = { version = "1", features = ["derive"] }
toml           = "0.8"             # config file
fuzzy-matcher  = "0.3"             # fuzzy session search
directories    = "5"               # XDG config/data paths
```

---

## Configuration File (`~/.config/mux/config.toml`)

```toml
[ui]
tree_width_percent = 28        # sidebar width as % of terminal
show_pane_titles   = true
show_window_count  = true
theme              = "dark"    # dark | light | auto

[keys]
passthrough_enter  = "ctrl-b ctrl-b"   # enter PtyPassthrough mode
passthrough_exit   = "ctrl-b ctrl-b"   # exit PtyPassthrough mode
new_session        = "n"
kill_session       = "D"
rename             = "r"
fuzzy_search       = "/"
command_mode       = ":"

[performance]
relay_buffer_limit_kb   = 64   # pause pane output above this threshold
control_read_buf_size   = 4096
max_relay_fps           = 0    # 0 = unlimited (outer terminal limits it)

[startup]
auto_attach = true             # attach to existing session if one exists
default_session = "main"
```

---

## Error Handling & Edge Cases

| Scenario | Handling |
|---|---|
| tmux server not running | Spawn new server; show inline notice in status bar |
| Session killed externally | `%session-closed` event → remove from tree, refocus |
| Terminal resize | Send `SIGWINCH` to child; recalculate layout rects; update relay region |
| Relay region overlap with tree | Tree always wins; relay region clipped to preview `Rect` only |
| tmux version < 2.9 | Check on startup (`tmux -V`); exit with error if unsupported |
| SSH / remote terminal | Detect `$SSH_TTY`; warn that relay region may have artifacts; offer focus handoff fallback |
| `%extended-output` flood | Flow-control via `%pause`/`%continue` at `relay_buffer_limit_kb` |

---

## MVP Scope (v0.1.0)

**In scope:**
- Tree view: sessions → windows → panes, expand/collapse
- `%extended-output` raw relay for focused pane preview
- Full session/window CRUD via control mode commands
- `PtyPassthrough` input mode (`send-keys` forwarding)
- Fuzzy session search overlay
- TOML config file
- Vim-style navigation keybindings
- Status bar with session/window/pane counts
- Dark/light theme tokens

**Deferred to v0.2.0:**
- Zoxide integration for session creation from directories
- Workspace snapshots (save/restore session layouts)
- Multiplexer agnosticism (Zellij backend trait)
- Mouse support
- `--json` machine-readable output mode
- Plugin system

