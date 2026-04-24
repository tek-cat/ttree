# ttree

A terminal UI for managing tmux sessions, written in Rust with [Ratatui](https://ratatui.rs).

Provides tree-based navigation (sessions → windows → panes) with **live embedded pane preview** — you can see what's running in any pane without switching to it.

Built to solve a specific problem: managing multiple concurrent AI agent sessions on a laptop from a phone running Termux over WireGuard VPN, where switching tmux contexts manually is slow and error-prone.

## Features

- Tree view of all sessions, windows, and panes with smart collapsing
- Live pane preview (embedded PTY, rendered in the right-hand panel)
- Click any session in the sidebar to switch the preview — works without leaving preview mode
- Persistent UI state (selection, expanded nodes, sidebar width) across restarts
- Full mouse support: click to select, drag sidebar to resize, scroll to navigate
- Vim-style navigation (`j`/`k`/`h`/`l`)
- Create and rename sessions without leaving the TUI
- Real-time sync with tmux every 200ms

## Stack

- **Rust 2021** + Cargo
- [ratatui](https://ratatui.rs) — TUI framework
- [tokio](https://tokio.rs) — async runtime
- [crossterm](https://docs.rs/crossterm) — terminal event handling
- [portable-pty](https://docs.rs/portable-pty) — PTY spawning for live preview
- [vt100](https://docs.rs/vt100) + [tui-term](https://docs.rs/tui-term) — VT100 parsing and rendering
- [indexmap](https://docs.rs/indexmap) — ordered session/window/pane collections

## Install

```bash
git clone git@gitlab.com:tek.cat/ttree.git
cd ttree
cargo install --path .
```

The binary lands in `~/.cargo/bin/ttree`. Requires Rust and an active tmux server.

## Usage

Run `ttree` from any terminal (inside or outside tmux).

### Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `h` / `←` | Collapse node / jump to parent |
| `l` / `→` | Expand node / jump to first child |
| `Space` | Toggle expand/collapse |
| `Enter` | Open live preview panel |
| `a` | Attach to selected session/window/pane |
| `n` | Create new session |
| `r` | Rename selected session |
| `Ctrl+P` | Toggle focus between tree and preview |
| `?` | Help |
| `q` / `Ctrl+C` | Quit |

In preview mode, `Ctrl+B` acts as the tmux prefix (e.g. `Ctrl+B d` to detach and return to tree).

## History

This repo also contains `tmux_session_manager.py`, an earlier Python/[Textual](https://textual.textualize.io) prototype with AI agent activity detection (classifies tmux windows as idle/working/needs-attention). The Rust rewrite (ttree) focuses on session management UX; the Python prototype's agent-detection logic is separate tooling.
