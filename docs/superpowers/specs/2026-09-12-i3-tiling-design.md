# Design: i3-style tiling manager for ttree

Date: 2026-09-12
Status: approved for planning (design presented in chat; no changes requested)
Related: [`docs/explorations/2026-09-10-i3-tiling-direction.md`](../../explorations/2026-09-10-i3-tiling-direction.md) - the spike that produced the architecture decision below.

## Decisions carried in from the spike

- **Mechanism:** tmux control mode (`tmux -C`). One control client streams `%output` for
  every pane in every window of the attached session at once, which is what makes
  compositing many tmux windows into one screen possible.
- **Structure:** a Cargo workspace. A shared `ttree-core` lib crate, the existing `ttree`
  binary (tree browser + preview, unchanged), and a new tiling binary (placeholder name
  `ttree-tile`).
- **Scope:** plan the full vision, delivered in phased milestones.
- **Relationship to the existing app:** the tree browser and live preview are untouched.
  Tiling is a new, separate mode / binary sharing the same tmux plumbing, not a
  replacement.

## Goals

- Arbitrarily nested containers of live tmux panes: `SplitH`, `SplitV`, `Tabbed`,
  `Stacked`, nesting without limit, matching i3's container model.
- Mouse support: click to focus, drag a border to resize, drag a pane to re-parent it
  elsewhere in the tree.
- Animated transitions (open, close, tab switch, drag preview, resize) rendered through
  ratatui, using frozen-frame chrome animation rather than live PTY resizing per frame.
- tmux stays the server: sessions persist and survive ttree-tile restarting; other tmux
  clients can still attach to any individual window.

## Non-goals (v1)

- Replacing ttree's tree browser or preview.
- ttree becoming its own multiplexer/daemon (architecture C from the spike) - only a
  fallback if control mode proves inadequate.
- A web client, a plugin system, multiplayer, Windows support.
- Multiple named workspaces (i3 has these; plausible stretch goal, not v1).

## Workspace restructure (milestone M0)

Convert the repo to a Cargo workspace:

- **`ttree-core`** (lib crate): `tmux_client.rs` (extended with a control-mode client),
  `theme.rs`, config-dir helpers, and the input layer currently embedded in `ttree`'s
  `main.rs` - SGR mouse decoding, the forwarding gate, the OSC 52 scanner, key encoding
  (see CLAUDE.md's note on where these are tested today). This is a pure extraction: same
  logic, same tests, moved so both binaries can use it.
- **`ttree`** (binary): today's tree browser + embedded-PTY preview. No behavior change.
- **`ttree-tile`** (binary, name a placeholder): the new tiling manager described below.

M0 ships alone, verified by `cargo clippy --all-targets --locked -- -D warnings` and
`cargo test` staying green with unchanged `ttree` behavior, before any tiling feature
code is written.

## Core data model

```rust
type LeafId = u64;
type ContainerId = u64;

enum Layout { SplitH, SplitV, Tabbed, Stacked }

struct Container {
    id: ContainerId,
    layout: Layout,
    children: Vec<(Node, Weight)>, // Weight: f32, proportional share like i3's percent
    focused_child: usize,
}

enum Node {
    Leaf(LeafId),
    Container(Container),
}

struct Leaf {
    id: LeafId,
    tmux_window_id: String, // "@123"
    parser: VtParser,       // vt100, or a replacement evaluated in the risk list below
    rect: ratatui::layout::Rect,
    title: String,
}
```

The root is a single `Container` (a "workspace" in i3 terms; multiple workspaces are a
stretch goal). Weights drive proportional resize. Tree mutation (split, move, close,
retype a container as tabbed/stacked) is pure functions over this structure and is unit
tested without tmux, the same way `state.rs` and `theme.rs` are tested today.

## tmux control-mode engine

- One dedicated tmux session, created if missing, with `window-size manual` set and the
  previous value saved/restored on exit - the same pattern ttree already uses for
  `mouse` and `set-clipboard` around the preview.
- One tmux window per leaf. `new-window -d -t <session> -P -F '#{window_id}'` to create
  a leaf (detached, so it never steals the terminal), `kill-window` to remove one.
- Every time the layout engine recomputes rects (resize, drag, tree mutation), diff
  against last-known per-leaf sizes and call `resize-window -t @id -x W -y H` only where
  something actually changed.
- A background task owns the control-mode client's stdout: parses `%output %<id> <data>`
  (decoding the `\xxx` octal + `<32` escaping), `%layout-change`, `%window-close`,
  `%exit`, and routes decoded bytes to the matching leaf's parser.
- Input to the focused leaf: `send-keys -H <hex>` for keys, `send-keys -M` (or a raw SGR
  sequence via `-H`) for mouse, translated to the leaf's local coordinates first.
- Flow control (`refresh-client -f pause-after=<n>`) is deferred until the throughput
  spike below says it's needed.

## Rendering / compositor

Ratatui's `Layout` is flat rows/cols and can't express i3-style nesting directly, so the
tree walk is custom: `SplitH`/`SplitV` allocate a rect to each child by weight and
recurse; `Tabbed`/`Stacked` render the focused child in the full rect plus a header (tab
strip, or a stacked title list). Each leaf blits its VT parser's screen into its rect via
tui-term. Chrome (borders, headers, focus ring) reuses `theme.rs`'s existing style
parsing so it matches the user's tmux status-bar colors like the rest of ttree does.

Effects are layered on top with **tachyonfx** (`ratatui/tachyonfx`): slide for tab
switches, dissolve/coalesce for close/open, a pulse for the drag drop-zone overlay. Any
animation that would otherwise require resizing a live PTY every frame instead animates
a per-leaf **frozen buffer** (a `Vec<Cell>` + rect snapshot taken before the animation
starts); the real `resize-window` is sent once, when the animation settles.

## Input & drag-and-drop

Keyboard nav is modeled on i3 (focus by direction, move a leaf by direction, set the new
split's orientation, retype a container as tabbed/stacked, close). The exact keymap is an
implementation-plan detail, not an architectural one - it doesn't affect the data model
or engine.

Mouse: click focuses a leaf. Dragging a leaf's border adjusts sibling weights live
(chrome-only during the drag, exactly like the sidebar's existing separator-drag), with
the real `resize-window` calls sent on mouse-up. Dragging a leaf itself starts a
re-parent: hovering an outer third of a target leaf previews a split in that direction,
hovering the center previews joining the target's tab/stack group (creating one if the
target is a bare leaf); dropping mutates the tree and animates into the new layout.

## Persistence

The container tree (layout kind, weights, and enough per-leaf information - the
originating command, or "shell" - to respawn a leaf that's gone) is saved to
`~/.config/ttree/tile-layout-<name>.toml`. TOML to match `state.toml`'s existing
convention rather than adopting Zellij's KDL. On start, ttree-tile reconciles the saved
tree against whatever windows are still alive in the dedicated tmux session before
spawning what's missing.

## Phased roadmap

| Milestone | Delivers | Gate before starting |
|---|---|---|
| M0 | Workspace split, `ttree-core` extraction, zero behavior change | - |
| M1 | Control-mode client to a dedicated session; hardcoded 2-leaf `SplitH`; keyboard nav only | Spike: independent per-window sizing under one control client (risk 1); input latency of `send-keys` vs. a direct PTY (risk 2) |
| M2 | Full recursive tree, N leaves, weighted resize, open/close a leaf | Spike: `%output` throughput with several busy panes at once (risk 3) |
| M3 | Persistence: save and reload a layout | - |
| M4 | Mouse: focus, border-drag resize, input passthrough to the focused leaf | - |
| M5 | Drag-and-drop re-parenting with the drop-zone overlay | - |
| M6 | tachyonfx animations + frozen-buffer resize | - |

The three spikes are the ones identified in the exploration doc; each is a cheap,
throwaway probe (tens of minutes), not a milestone deliverable in itself.

## Testing strategy

- Tree mutation (split / move / close / retype) and rect-from-weights: pure functions,
  unit tested with no tmux, next to the tree module - matching how `state.rs` and
  `theme.rs` are tested today.
- The control-mode line parser (`%output` / `%layout-change` / `%window-close` framing
  and escape decoding): pure, tested against captured fixture lines.
- Everything touching a live control client needs a real tmux server and is exercised
  manually, the same limitation the existing preview already has (per CLAUDE.md).

## Error handling

- A leaf's tmux window disappears out-of-band (killed by another client): detected via
  `%window-close`, the leaf becomes a "dead pane" placeholder showing the exit status
  with an offer to respawn or close it.
- The control-mode client itself exits (`%exit`, or the process dies): the tiling view
  shows a reconnect banner and retries with backoff.
- Two rect recomputations race ahead of tmux's acknowledgment: coalesce to the latest
  rect only, never queue stale resizes.

## Open items intentionally left to the implementation plan

- Final binary/crate name (`ttree-tile` is a placeholder).
- The exact keymap.
- Whether `ttree` gains a way to jump into `ttree-tile` directly (e.g. a keybind that
  execs it against the same session) versus the two being launched independently.
- VT-parser choice if `vt100` proves too limited on scrollback (evaluate `avt` or
  `wezterm-term` - noted as risk 4 in the exploration doc).
