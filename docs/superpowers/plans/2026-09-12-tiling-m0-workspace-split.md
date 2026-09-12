# Tiling M0: Workspace Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the `ttree` repo into a Cargo workspace - a shared `ttree-core` library crate plus the existing `ttree` binary, unchanged in behavior - so the tiling manager (`ttree-tile`, M1+) has somewhere to depend on shared tmux/theme/input plumbing without duplicating it.

**Architecture:** `ttree-core` becomes a new lib crate holding everything that isn't specific to the tree-browser UI: tmux read/write commands (`tmux_client.rs`, `actions.rs`), the tmux-status-bar theme (`theme.rs`), config-directory resolution (new `config.rs`), and the terminal input-encoding layer currently embedded in `ttree`'s `main.rs` (new `input.rs`: SGR mouse encoding, the forwarding gate, the OSC 52 scanner, key encoding, and the tmux-prefix matcher). `ttree`'s own `main.rs`, `state.rs`, and `ui/` stay exactly where they are and keep their current behavior; they just import from `ttree_core` instead of `crate` for the moved pieces. No new features, no behavior change - this milestone exists purely so later milestones have a crate to build `ttree-tile` on top of.

**Tech Stack:** Rust 2021, Cargo workspaces (`resolver = "2"`, `[workspace.dependencies]`). No new external dependencies.

**Spec:** `docs/superpowers/specs/2026-09-12-i3-tiling-design.md` (see "Workspace restructure (milestone M0)") and `docs/explorations/2026-09-10-i3-tiling-direction.md` for why this direction was chosen.

## Global Constraints

- Branch: `tiling` (already checked out; work directly on it, commit after every task).
- Lint with the exact CI line: `cargo clippy --all-targets --locked -- -D warnings` (a bare `cargo clippy` skips test code and passes on things CI fails - see `CLAUDE.md`).
- After every task that changes what gets built, run `cargo install --path .` from the repo root so `~/.cargo/bin/ttree` reflects the change (existing project convention).
- **Do not touch `/home/user/Projects/ttree/ttree/`** (note the trailing path segment - this is a stray, `.gitignore`d directory left over from a 2026 "flatten Rust to root" restructuring; it holds a stale `Cargo.lock` and a 2.5 MB `ttree.log`, is untracked by git, and is unrelated to this plan). The workspace member crates created below use different directory names specifically to avoid colliding with it.
- No behavior change to the `ttree` binary is permitted in this milestone. Every task must leave `cargo test` and `cargo clippy --all-targets --locked -- -D warnings` green.
- End every commit message with:
  ```
  Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01GvsZA7jRTkzKGwNA3PoZDz
  ```
- No em dashes in commit messages, code comments, or docs.

---

### Task 1: Workspace skeleton

**Files:**
- Modify: `Cargo.toml` (repo root)
- Create: `ttree-core/Cargo.toml`
- Create: `ttree-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing yet.
- Produces: a `ttree-core` crate that exists, builds, and exports nothing (`ttree-core/src/lib.rs` is empty). Later tasks add `pub mod` declarations to it.

- [ ] **Step 1: Rewrite the root `Cargo.toml` as a workspace root + package**

Replace the entire file with:

```toml
[workspace]
members = ["ttree-core"]
resolver = "2"

[workspace.package]
version = "0.2.0"
edition = "2021"
license = "MIT"

[workspace.dependencies]
ratatui        = "0.30.0"
crossterm      = "0.28"
tokio          = { version = "1", features = ["full"] }
indexmap       = "2"
anyhow         = "1"
clap           = { version = "4", features = ["derive"] }
serde          = { version = "1", features = ["derive"] }
toml           = "0.8"
fuzzy-matcher  = "0.3"
directories    = "5"
tui-term       = "0.3.4"
vt100          = "0.16.2"
portable-pty   = "0.9.0"
ttree-core     = { path = "ttree-core" }

[package]
name = "ttree"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "A tmux session manager TUI: tree navigation with a live embedded pane preview."
repository = "https://gitlab.com/tek.cat/ttree"
homepage = "https://ttree.tek.cat"
documentation = "https://gitlab.com/tek.cat/ttree/-/blob/main/README.md"
readme = "README.md"
keywords = ["tmux", "tui", "terminal", "ratatui", "session-manager"]
categories = ["command-line-utilities"]
exclude = ["demo/", "docs/", "public/", "scripts/", ".gitlab-ci.yml"]

[dependencies]
ratatui        = { workspace = true }
crossterm      = { workspace = true }
tokio          = { workspace = true }
indexmap       = { workspace = true }
anyhow         = { workspace = true }
clap           = { workspace = true }
serde          = { workspace = true }
toml           = { workspace = true }
fuzzy-matcher  = { workspace = true }
directories    = { workspace = true }
tui-term       = { workspace = true }
vt100          = { workspace = true }
portable-pty   = { workspace = true }
```

(`directories` stays a direct `ttree` dependency for now - `state.rs` still calls `directories::ProjectDirs` itself until Task 4. It is removed from this list in Task 4.)

- [ ] **Step 2: Create the `ttree-core` crate**

`ttree-core/Cargo.toml`:

```toml
[package]
name = "ttree-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
```

`ttree-core/src/lib.rs`:

```rust
//! Shared plumbing for ttree's binaries: tmux read/write commands, the
//! tmux-status-bar theme, config-directory resolution, and the terminal
//! input-encoding layer. See `docs/superpowers/specs/2026-09-12-i3-tiling-design.md`
//! for why this crate exists.
```

- [ ] **Step 3: Verify nothing broke**

Run: `cargo build --workspace`
Expected: builds successfully, `target/debug/ttree` produced, `ttree-core` compiles as an (currently empty) library.

Run: `cargo test --workspace` and `cargo clippy --all-targets --locked -- -D warnings`
Expected: same pass/fail results as before this task (no test moved yet).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock ttree-core/
git commit -m "Add ttree-core as an empty workspace member"
```

---

### Task 2: Move `tmux_client.rs` and `actions.rs` into `ttree-core`

Both files are already fully self-contained (only `anyhow` and `tokio` dependencies, no `use crate::...` of their own), so this is a clean move with no internal adjustment beyond visibility, which is already `pub` throughout both files.

**Files:**
- Modify: `ttree-core/Cargo.toml`, `ttree-core/src/lib.rs`
- Create: `ttree-core/src/tmux_client.rs` (moved from `src/tmux_client.rs`)
- Create: `ttree-core/src/actions.rs` (moved from `src/actions.rs`)
- Delete: `src/tmux_client.rs`, `src/actions.rs`
- Modify: `Cargo.toml` (repo root), `src/main.rs`

**Interfaces:**
- Consumes: the `ttree-core` crate skeleton from Task 1.
- Produces: `ttree_core::tmux_client::Tmux` (same public API `Tmux` had at `crate::tmux_client::Tmux`) and `ttree_core::actions::Actions` (same public API `Actions` had at `crate::actions::Actions`). Task 3 (`theme.rs`) depends on `ttree_core::tmux_client::Tmux` existing.

- [ ] **Step 1: Move the files with git, preserving history**

```bash
git mv src/tmux_client.rs ttree-core/src/tmux_client.rs
git mv src/actions.rs ttree-core/src/actions.rs
```

Do not edit the content of either file - both compile as-is once they're reachable as `ttree-core` modules, since neither uses `crate::` internally.

- [ ] **Step 2: Wire them into `ttree-core`**

Add to `ttree-core/src/lib.rs`:

```rust
pub mod actions;
pub mod tmux_client;
```

Add to `ttree-core/Cargo.toml`'s `[dependencies]`:

```toml
anyhow = { workspace = true }
tokio  = { workspace = true }
```

- [ ] **Step 3: Point `ttree` at the moved modules**

In the root `Cargo.toml`, add to `ttree`'s `[dependencies]`:

```toml
ttree-core     = { workspace = true }
```

In `src/main.rs`, remove these two lines:

```rust
mod actions;
mod tmux_client;
```

and change:

```rust
use crate::actions::Actions;
```

to:

```rust
use ttree_core::actions::Actions;
```

and change:

```rust
use crate::tmux_client::Tmux;
```

to:

```rust
use ttree_core::tmux_client::Tmux;
```

`mod state;`, `mod theme;`, `mod ui;` and the `use crate::state::...` line stay untouched - those aren't moving in this task.

- [ ] **Step 4: Verify**

Run: `cargo test --workspace`
Expected: `actions.rs`'s existing `#[cfg(test)]` tests now run under `cargo test -p ttree-core` and pass unchanged; `ttree`'s own test suite (`cargo test -p ttree`) is unaffected.

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean.

Run: `cargo install --path .`
Expected: succeeds, `~/.cargo/bin/ttree` rebuilt.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Move tmux_client and actions into ttree-core"
```

---

### Task 3: Move `theme.rs` into `ttree-core`

`theme.rs` depends on `tmux_client::Tmux`, which Task 2 already placed in `ttree-core`, so its internal `use crate::tmux_client::Tmux;` resolves unchanged once it's a sibling module in the same crate.

**Files:**
- Modify: `ttree-core/src/lib.rs`
- Create: `ttree-core/src/theme.rs` (moved from `src/theme.rs`)
- Delete: `src/theme.rs`
- Modify: `src/main.rs`, `src/state.rs`

**Interfaces:**
- Consumes: `ttree_core::tmux_client::Tmux` from Task 2.
- Produces: `ttree_core::theme::Theme` (same public API `Theme` had at `crate::theme::Theme`), used by `state.rs`.

- [ ] **Step 1: Move the file**

```bash
git mv src/theme.rs ttree-core/src/theme.rs
```

No content changes needed - `use crate::tmux_client::Tmux;` inside it still resolves, because `tmux_client` is now a sibling module inside the same `ttree-core` crate.

- [ ] **Step 2: Wire it into `ttree-core`**

Add to `ttree-core/src/lib.rs`:

```rust
pub mod theme;
```

Add to `ttree-core/Cargo.toml`'s `[dependencies]`:

```toml
ratatui = { workspace = true }
```

(`theme.rs` uses `ratatui::style::Color`.)

- [ ] **Step 3: Update `ttree`'s references**

In `src/main.rs`, remove:

```rust
mod theme;
```

In `src/state.rs`, change both occurrences of `crate::theme::Theme` to `ttree_core::theme::Theme`:

```rust
pub theme: crate::theme::Theme,
```
becomes
```rust
pub theme: ttree_core::theme::Theme,
```

and

```rust
theme: crate::theme::Theme::default(),
```
becomes
```rust
theme: ttree_core::theme::Theme::default(),
```

- [ ] **Step 4: Verify**

Run: `cargo test --workspace`
Expected: `theme.rs`'s existing `#[cfg(test)]` tests now run under `cargo test -p ttree-core` and pass unchanged.

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Move theme into ttree-core"
```

---

### Task 4: Extract config-directory resolution into `ttree-core::config`

`state.rs` currently calls `directories::ProjectDirs::from("com", "tek", "ttree")` in two places (`save_to_disk` and `load_from_disk`). `ttree-tile` will need the same resolution for its own layout file later (per the spec's persistence section), so it becomes a one-line shared helper now rather than a second copy-paste later.

**Files:**
- Modify: `ttree-core/src/lib.rs`
- Create: `ttree-core/src/config.rs`
- Modify: `src/state.rs`
- Modify: `Cargo.toml` (repo root)

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub fn ttree_core::config::config_dir() -> Option<std::path::PathBuf>`.

- [ ] **Step 1: Add the helper**

Create `ttree-core/src/config.rs`:

```rust
//! Where ttree's own config and state live on disk, e.g. `~/.config/ttree` on
//! Linux. Shared by every ttree binary so they agree on the same directory.

use std::path::PathBuf;

/// `None` only if the platform gives us no resolvable home/config directory.
pub fn config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "tek", "ttree")
        .map(|proj_dirs| proj_dirs.config_dir().to_path_buf())
}
```

Add to `ttree-core/src/lib.rs`:

```rust
pub mod config;
```

Add to `ttree-core/Cargo.toml`'s `[dependencies]`:

```toml
directories = { workspace = true }
```

- [ ] **Step 2: Use it from `state.rs`**

In `src/state.rs`, in `save_to_disk`, change:

```rust
    pub fn save_to_disk(&self) {
        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "tek", "ttree") {
            let config_dir = proj_dirs.config_dir();
            let _ = std::fs::create_dir_all(config_dir);
            let state_path = config_dir.join("state.toml");
```

to:

```rust
    pub fn save_to_disk(&self) {
        if let Some(config_dir) = ttree_core::config::config_dir() {
            let _ = std::fs::create_dir_all(&config_dir);
            let state_path = config_dir.join("state.toml");
```

In `load_from_disk`, change:

```rust
    pub fn load_from_disk() -> Self {
        let mut state = Self::default();
        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "tek", "ttree") {
            let state_path = proj_dirs.config_dir().join("state.toml");
```

to:

```rust
    pub fn load_from_disk() -> Self {
        let mut state = Self::default();
        if let Some(config_dir) = ttree_core::config::config_dir() {
            let state_path = config_dir.join("state.toml");
```

- [ ] **Step 3: Drop the now-unused direct dependency from `ttree`**

In the root `Cargo.toml`, remove this line from `ttree`'s `[dependencies]`:

```toml
directories    = { workspace = true }
```

(`directories` stays in `[workspace.dependencies]` and in `ttree-core`'s own `[dependencies]` - only the unused direct dependency on `ttree` goes away.)

- [ ] **Step 4: Verify**

Run: `cargo build --workspace`
Expected: builds clean (this also confirms `ttree` no longer references `directories` directly - if it still did, this step would fail to compile once the dependency is removed).

Run: `cargo clippy --all-targets --locked -- -D warnings` and `cargo test --workspace`
Expected: clean, all green.

Manual check (no automated test covers this path today): run `cargo run`, resize the sidebar or toggle a node open, quit, and run again - the sidebar width/expanded state should still persist across the restart, confirming `save_to_disk`/`load_from_disk` still round-trip through `~/.config/ttree/state.toml`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Share config-directory resolution through ttree-core"
```

---

### Task 5: Extract the input-encoding layer into `ttree-core::input`

This is the block CLAUDE.md refers to as "the input layer (SGR mouse encoding, the forwarding gate, the OSC 52 scanner, key encoding)". As of this plan's writing it is one contiguous block in `src/main.rs` from the line `fn encode_mouse(` through the closing brace of `fn encode_key`, currently lines 297-686 - but locate it by content, not by that line number, since Tasks 2-4 already shifted a handful of lines near the top of the file. Its tests are the `mod tests` and `mod prefix_tests` blocks at the end of the file (currently lines 2102-2278), which are self-contained (`use super::*;` plus `crossterm::event` only - nothing from `state.rs` or `theme.rs`).

Every function and struct in this block is either `pub` already or needs to become `pub` because it's now called from a different crate. Field-level visibility only matters for `Prefix`, because `src/main.rs` reads `prefix.byte` directly (`main.rs:2039` today) - `Osc52Relay`'s fields are only ever touched through its own methods, so they stay private.

**Files:**
- Modify: `ttree-core/src/lib.rs`
- Create: `ttree-core/src/input.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `pub fn ttree_core::input::encode_mouse(mouse: &crossterm::event::MouseEvent, x_offset: u16, y_offset: u16) -> Option<Vec<u8>>`
  - `pub fn ttree_core::input::should_forward_mouse(kind: &crossterm::event::MouseEventKind, mode: vt100::MouseProtocolMode) -> bool`
  - `pub fn ttree_core::input::encode_key(key: &crossterm::event::KeyEvent, bytes: &mut Vec<u8>)`
  - `pub struct ttree_core::input::Osc52Relay` (derives `Default`) with `pub fn feed(&mut self, data: &[u8])`
  - `pub struct ttree_core::input::Prefix { pub ch: char, pub byte: u8 }` (derives `Clone, Copy, Debug, PartialEq`, plus `Default`) with `pub fn parse(value: Option<String>) -> Self` and `pub fn matches(&self, key: &crossterm::event::KeyEvent) -> bool`

- [ ] **Step 1: Locate the exact block**

```bash
grep -n "^fn encode_mouse(\|^fn encode_key(\|^mod tests\|^mod prefix_tests" src/main.rs
```

Confirm one contiguous span from `fn encode_mouse(` to the end of `fn encode_key` (ending `_ => {}\n    }\n}`), and a separate contiguous span covering `mod tests { ... }` followed immediately by `mod prefix_tests { ... }` at the end of the file.

- [ ] **Step 2: Create `ttree-core/src/input.rs`**

Read `src/main.rs` from `fn encode_mouse(` through the end of `fn encode_key`'s closing brace (found in Step 1) and copy that block verbatim into a new `ttree-core/src/input.rs`, then apply exactly these changes to what you pasted:

1. Add these two lines at the top of the new file, before the copied block:

```rust
//! The terminal input-encoding layer: SGR mouse encoding, the forwarding
//! gate, the OSC 52 clipboard scanner, key encoding, and the tmux-prefix
//! matcher. Moved out of `ttree`'s `main.rs` so `ttree-tile` can reuse it -
//! see `docs/superpowers/specs/2026-09-12-i3-tiling-design.md`.

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use std::io;
```

2. Add `pub` to the three top-level functions that don't already have it:
   - `fn encode_mouse(` -> `pub fn encode_mouse(`
   - `fn should_forward_mouse(` -> `pub fn should_forward_mouse(`
   - `fn encode_key(` -> `pub fn encode_key(`

   (`modifier_code`, `csi_letter`, `csi_tilde` stay private - they're internal helpers `encode_key` calls, never used outside this file.)

3. Add `pub` to the two structs:
   - `struct Osc52Relay {` -> `pub struct Osc52Relay {`
   - `struct Prefix {` -> `pub struct Prefix {`

4. Add `pub` to `Prefix`'s two fields:

```rust
pub struct Prefix {
    /// The letter, for matching a key event.
    pub ch: char,
    /// The control byte a terminal actually sends for it, for forwarding.
    pub byte: u8,
}
```

5. Add `pub` to `Osc52Relay::feed` and both of `Prefix`'s methods:
   - `fn feed(&mut self, data: &[u8]) {` -> `pub fn feed(&mut self, data: &[u8]) {`
   - `fn parse(value: Option<String>) -> Self {` -> `pub fn parse(value: Option<String>) -> Self {`
   - `fn matches(&self, key: &crossterm::event::KeyEvent) -> bool {` -> `pub fn matches(&self, key: &crossterm::event::KeyEvent) -> bool {`

   (`Osc52Relay::scan` and `Osc52Relay::emit` stay private - `scan` is only called by `feed` and by this file's own tests via `use super::*;`, `emit` is only called by `feed`.)

Then append the two test modules (`mod tests { ... }` and `mod prefix_tests { ... }`, found in Step 1) verbatim to the end of `ttree-core/src/input.rs`, unchanged - they already only depend on `super::*` and `crossterm::event`.

- [ ] **Step 3: Delete the block from `src/main.rs`**

Delete, from `src/main.rs`:
- The block from `fn encode_mouse(` through the end of `fn encode_key` (now duplicated into `ttree-core/src/input.rs`).
- The `mod tests { ... }` and `mod prefix_tests { ... }` blocks at the end of the file (now moved into `ttree-core/src/input.rs`).

- [ ] **Step 4: Wire the new module in and fix call sites**

Add to `ttree-core/src/lib.rs`:

```rust
pub mod input;
```

Add to `ttree-core/Cargo.toml`'s `[dependencies]`:

```toml
crossterm = { workspace = true }
vt100     = { workspace = true }
```

In `src/main.rs`, add this import alongside the other `use ttree_core::...` lines:

```rust
use ttree_core::input::{encode_key, encode_mouse, should_forward_mouse, Osc52Relay, Prefix};
```

`src/main.rs`'s existing top-of-file `use crossterm::event::{...}` list (`KeyCode, KeyModifiers, MouseButton, MouseEventKind`, etc.) stays exactly as it is - the main loop still uses those names directly for its own, non-moved code. No other call site changes are needed: `Osc52Relay::default()`, `clipboard.feed(...)`, `Prefix::parse(...)`, `prefix.byte`, `prefix.matches(...)`, `encode_mouse(...)`, `should_forward_mouse(...)`, and `encode_key(...)` all resolve unchanged once the names above are imported, since none of those call sites were written with a `crate::` or module-qualified prefix.

- [ ] **Step 5: Verify**

Run: `cargo test --workspace`
Expected: every test that was passing under `ttree`'s `mod tests`/`mod prefix_tests` before this task now passes under `cargo test -p ttree-core` with identical names and assertions - `mouse_encodes_sgr_with_one_based_coordinates`, `mouse_release_uses_lowercase_terminator`, `mouse_drag_and_wheel_use_their_own_button_codes`, `mouse_modifiers_add_their_bits`, `forwarding_follows_the_mode_the_client_asked_for`, `bare_hover_is_never_forwarded`, `osc52_relays_a_bel_terminated_sequence`, `osc52_relays_an_st_terminated_sequence`, `osc52_survives_every_split_point`, `osc52_ignores_other_sequences`, `osc52_restarts_the_introducer_on_a_repeated_escape`, `osc52_keeps_an_escape_that_is_not_a_terminator`, `osc52_drops_an_oversized_payload_and_recovers`, `keys_encode_plain_and_modified_forms`, `prefix_follows_the_tmux_option`, `unparseable_prefixes_fall_back_to_c_b`, `prefix_matches_only_with_control`.

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean. Pay particular attention to `dead_code`/`unused` warnings on `ttree` if any call site was missed.

Run: `cargo install --path .`
Expected: succeeds.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Extract the input-encoding layer into ttree-core"
```

---

### Task 6: Update docs and final verification

**Files:**
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: the finished workspace from Tasks 1-5.
- Produces: nothing code-facing; this is the milestone's closing checkpoint.

- [ ] **Step 1: Update `CLAUDE.md`'s Architecture section**

In `/home/user/Projects/ttree/CLAUDE.md`, replace the paragraph introducing the "Architecture" section (the one starting "ttree is a single-binary Rust TUI...") with:

```markdown
ttree is a Cargo workspace: a shared `ttree-core` library crate, and the `ttree`
binary (`src/`) built on ratatui + tokio. `ttree-core` holds the tmux client,
the tmux-status-bar theme, config-directory resolution, and the terminal
input-encoding layer (SGR mouse encoding, the forwarding gate, the OSC 52
scanner, key encoding, the tmux-prefix matcher) - everything a second binary
(the i3-style tiling manager planned in
`docs/superpowers/specs/2026-09-12-i3-tiling-design.md`) needs without
duplicating it. `ttree` itself holds everything specific to the tree-browser
UI: the main loop, app state, and rendering.
```

Update the line about where input-layer tests live - change:

```markdown
Tests are `#[cfg(test)]` modules beside the code they cover: the input layer
(SGR mouse encoding, the forwarding gate, the OSC 52 scanner, key encoding) in
`main.rs`, tree flattening and selection in `state.rs`, style parsing in
`theme.rs`.
```

to:

```markdown
Tests are `#[cfg(test)]` modules beside the code they cover: the input layer
(SGR mouse encoding, the forwarding gate, the OSC 52 scanner, key encoding) in
`ttree-core/src/input.rs`, tree flattening and selection in `src/state.rs`,
style parsing in `ttree-core/src/theme.rs`.
```

Update the numbered data-flow list (`tmux_client.rs`, `state.rs`, `main.rs`, `ui/mod.rs`, `ui/tree.rs`) to note the first two now live under `ttree-core/src/`:

```markdown
1. **`ttree-core/src/tmux_client.rs`** - thin async wrapper around `tmux` CLI. ...
2. **`src/state.rs`** - parses those strings into `AppState` ... Also owns
   focus/scroll/input state and persists a subset to `~/.config/ttree/state.toml`
   via serde+toml (through `ttree_core::config::config_dir`).
```

(Keep the rest of that list's wording; only the two path prefixes change.)

- [ ] **Step 2: Full verification pass**

```bash
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace
cargo install --path .
```

Expected: all three succeed with no warnings and no failing tests.

Manual smoke test (no automated coverage exists for the live tmux interaction, per `CLAUDE.md`): with a tmux server running and at least one other session open, run `ttree`, confirm the tree renders, press `Enter` on a session to open the live preview, confirm it shows real output, click around in the sidebar and the preview to confirm mouse handling still works, then quit.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "Document the ttree-core workspace split"
```

This closes milestone M0. M1 (the control-mode client and the first hardcoded tiling layout, per the spec's phased roadmap) is planned separately once M0 is merged.
