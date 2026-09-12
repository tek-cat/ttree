# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                  # debug build
cargo build --release        # release build
cargo install --path .       # install binary to ~/.cargo/bin/ttree (do this after every code change)
cargo check                  # fast type-check without linking
cargo test --workspace        # unit tests
cargo fmt --check             # what CI runs (formatting gate)
cargo clippy --workspace --all-targets --locked -- -D warnings   # what CI runs
```

Lint with that exact clippy line. A bare `cargo clippy` skips test code, so it
passes on things CI fails.

Tests are `#[cfg(test)]` modules beside the code they cover: the input layer
(SGR mouse encoding, the forwarding gate, the OSC 52 scanner, key encoding) in
`ttree-core/src/input.rs`, tree flattening and selection in `src/state.rs`,
style parsing in `ttree-core/src/theme.rs`. They are pure functions and need no
tmux. Everything else does require a live tmux server, and the preview in
particular can only be checked by driving a real PTY.

## Architecture

ttree is a Cargo workspace: a shared `ttree-core` library crate, and the `ttree`
binary (`src/`) built on ratatui + tokio. `ttree-core` holds the tmux client,
the tmux-status-bar theme, config-directory resolution, and the terminal
input-encoding layer (SGR mouse encoding, the forwarding gate, the OSC 52
scanner, key encoding, the tmux-prefix matcher) - everything a second binary
(the i3-style tiling manager planned in
`docs/superpowers/specs/2026-09-12-i3-tiling-design.md`) needs without
duplicating it. `ttree` itself holds everything specific to the tree-browser
UI: the main loop, app state, and rendering.

### Data flow

1. **`ttree-core/src/tmux_client.rs`** - thin async wrapper around `tmux` CLI. All three queries (`list-sessions`, `list-windows`, `list-panes`) use U+001F as a field separator and return raw strings.
2. **`src/state.rs`** - parses those strings into `AppState` (IndexMaps of `Session`, `Window`, `Pane`). Also owns focus/scroll/input state and persists a subset to `~/.config/ttree/state.toml` via serde+toml (through `ttree_core::config::config_dir`).
3. **`src/main.rs`** - the main loop: syncs state every 200 ms, handles all keyboard/mouse events, spawns the embedded PTY (`portable-pty`) and drives the `vt100` parser for preview rendering.
4. **`src/ui/mod.rs`** - ratatui render function. Layout: full-height sidebar (tree) + fill preview, plus a 1-row command bar at the bottom.
5. **`src/ui/tree.rs`** - stateless `TreeWidget` that renders the session tree with three display cases: single-leaf (no hierarchy), single-window session (session header + panes), full hierarchy (session → windows → panes).

### Key design decisions

- **Selection is always an ID string** (`$session_id`, `@window_id`, or `%pane_id`). The nav mode (`Session`/`Window`/`Pane`) is tracked separately but the visible list (`get_dynamic_visible_items`) is the source of truth for what's selectable.
- **Preview is a real embedded tmux client** spawned via `tmux attach-session -t <id>` inside a portable-pty pair with `TMUX`/`TMUX_PANE` unset to avoid nesting. The vt100 parser feeds `tui-term`'s `PseudoTerminal` widget for rendering. Switching sessions in preview mode uses `tmux switch-client` against the embedded client's TTY.
- **Input modes** (`TuiNormal`, `Renaming`, `NewSession`) are an enum in `state.rs`; the main loop checks the active mode before dispatching key events.
- **Mouse**: separator drag has highest priority, then sidebar clicks (work in both Tree and Preview panel focus), then passthrough to the embedded PTY for events in the preview area.
- **`public/`** is a static GitLab Pages site. It's deployed by `.gitlab-ci.yml` and is not related to the Rust build.
