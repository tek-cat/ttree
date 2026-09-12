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

- **Performance is an explicit hard requirement, not a nice-to-have.** Cheap re-layout on
  every frame/resize, cheap focus/move mutations, no needless allocation in the render hot
  path, and no wasted work on an idle tick. See "Core data model" below for the concrete
  rules this implies, drawn from researching i3's and sway's actual internals.
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

Informed by researching i3's and sway's actual internals for the performance
requirement below - full findings in
[`docs/explorations/2026-09-12-i3-sway-data-structures-research.md`](../../explorations/2026-09-12-i3-sway-data-structures-research.md).
Three decisions carry over directly: an arena with typed indices instead of pointers or
nested `Box`, a display-order list separate from a focus-order list (i3's `nodes_head` /
`focus_head` split), and weights as the persisted truth with rects recomputed top-down
every frame rather than stored as state (i3's `percent` vs. its explicitly-ephemeral
`rect`).

```rust
new_key_type! { struct NodeId; } // slotmap, or an equivalent Vec<Option<Node>> + free list

enum Layout { SplitH, SplitV, Tabbed, Stacked }

struct Container {
    layout: Layout,
    parent: Option<NodeId>,
    children: Vec<NodeId>,     // display order - a Vec, not a linked list: every layout
                                // pass iterates all of them anyway (sway's tradeoff)
    weights: Vec<f32>,         // parallel to children, renormalized to sum to 1.0 on
                                // attach/detach (i3's con_fix_percent)
    focus_order: Vec<NodeId>,  // same members as children, most-recently-focused first
                                // (i3's focus_head), updated by remove + push-front and
                                // bubbled to the parent on every focus change
    rect: Rect,                // last-committed rect, cached only so a resize can skip
                                // recursing into a subtree whose rect didn't change
}

struct Leaf {
    parent: Option<NodeId>,
    tmux_window_id: String,    // "@123"
    parser: VtParser,          // vt100, or a replacement evaluated in the risk list below
    rect: Rect,                // last rect actually pushed to tmux via resize-window
    generation: u64,           // bumped when the parser processes new bytes; compared
                                // against "last drawn" to skip re-blitting unchanged content
    title: String,
}

enum Node { Container(Container), Leaf(Leaf) }

struct Tree {
    nodes: SlotMap<NodeId, Node>,
    root: NodeId,
    focused: NodeId,           // O(1) "what's active" (i3's global `focused` pointer)
    dirty: Vec<NodeId>,        // flat, idempotent list appended to by every mutation,
                                // drained once per render tick (sway's server.dirty_nodes)
}
```

The root is a single `Container` (a "workspace" in i3 terms; multiple workspaces are a
stretch goal). Tree mutation (split, move, close, retype a container as tabbed/stacked)
is pure functions over this structure and is unit tested without tmux, the same way
`state.rs` and `theme.rs` are tested today. Every detach also prunes degenerate
single-child wrapper containers (i3's `tree_flatten`), so unlimited nesting doesn't
silently become unbounded wrapper accumulation.

**Performance rules, not just data shape** (see the research doc for why each one is
safe to rely on):
- Recompute every rect top-down from weights on every layout pass. This is cheap
  arithmetic over a small tree - i3's own position is "correct and simple beats
  incremental," and it's right here too.
- Never issue a `resize-window` (or any other tmux-facing call) for a leaf whose newly
  computed rect equals its cached one. This equality guard, not the ratatui geometry
  math, is where ttree-tile's actual cost lives.
- Skip the render tick entirely when `Tree.dirty` is empty and no leaf's `generation`
  changed since the last draw - an idle tiling view should do zero work, the same way an
  idle wlroots output does.

## tmux control-mode engine

- One dedicated tmux session, created if missing, with `window-size manual` set and the
  previous value saved/restored on exit - the same pattern ttree already uses for
  `mouse` and `set-clipboard` around the preview.
- One tmux window per leaf. `new-window -d -t <session> -P -F '#{window_id}'` to create
  a fresh leaf (detached, so it never steals the terminal); `move-window -s <src> -t
  <session>:` to import an *existing* window from elsewhere on the same server as a leaf
  instead, preserving its scrollback and running process - the mechanism that lets
  ttree-tile tile work that's already running, not just work it spawns itself (see
  "Isolation" below for why this requires staying on the same tmux socket). `kill-window`
  to remove a leaf.
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

### Isolation: ambient socket, dedicated session (not a dedicated socket)

ttree-tile runs on the **same tmux socket** the user's other sessions already live on,
in **one dedicated session** reserved for it (a fixed, recognizable name, e.g.
`ttree-tile`), not a separate server/socket. This is a deliberate choice, not the
default by omission:

- **A dedicated socket would forfeit the ability to tile pre-existing work.** tmux has
  no cross-server operations - `move-window` and `link-window` only operate within one
  server. If ttree-tile lived on its own socket, it could only tile windows it spawns
  itself; it could never pull in a session (a live Claude Code agent, say) already
  running on the user's normal server. Given ttree's whole premise is unifying view of
  already-running work, that would cut against the point. Staying on the ambient socket
  keeps `move-window`/`link-window` available as the mechanism for grabbing a live
  window into the tree without killing and respawning it (preserving scrollback and
  process state).
- **The dedicated session is still real isolation from the user's other sessions',**
  just at the session level rather than the server level: other sessions are never
  resized or reparented by ttree-tile, and `window-size manual` on this one session
  means the rest of the server's sessions are unaffected by it.
- A `--socket`/`-L` override, mirroring the flag ttree's browser already supports (`tmux
  -L name` / `-S /path`), is a cheap escape hatch for anyone who wants full isolation at
  the cost of "fresh workspace only" - not the default.

**What other viewers actually see**, since a plain tmux client and ttree-tile share a
server: `tmux ls` lists the dedicated session like any other. Attaching to it directly
with plain tmux shows one window at a time, pinned to whatever size ttree-tile last set
via `resize-window` regardless of the attaching client's own terminal size - a client
smaller than that window can force a "too small" state for anyone else attached, the
same class of bug the exploration doc flagged in iTerm2's tmux integration. The existing
`ttree` browser needs no special-casing at all: it already treats every session's
windows uniformly, so it can list and preview any individual ttree-tile leaf through its
normal grouped-mirror mechanism. What's invisible outside ttree-tile's own process in
every case is the i3 *structure* - tmux only ever sees a flat window list; the
split/tabbed/stacked nesting exists in the in-memory tree (and, from M3 on, the saved
layout file), never in tmux itself.

**Multiple ttree-tile instances against the same session is a real conflict, not a
hypothetical.** A tmux window has exactly one size at a time - `window-size manual`
controls how tmux picks that size, it does not give different attached clients
different sizes. Two control-mode clients computing layouts from two different terminal
sizes and both calling `resize-window` on the same windows would fight, each clobbering
the other's rects. The fix: on launch, check for the reserved session name; if it's
already running, a second instance attaches as a **non-authoritative viewer** (streams
`%output`, renders whatever size the first instance already chose, issues no
`resize-window` calls of its own) rather than becoming a second authority. Exactly one
instance is ever the layout authority for a given session. This is the same shape as the
`ttree` browser's existing preview mirroring, applied to the whole tiling session
instead of one pane.

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

- Arena crate choice for `Tree::nodes` (`slotmap` is the natural fit - typed keys,
  O(1) remove with reuse, no unsafe) versus a hand-rolled `Vec<Option<Node>>` + free
  list to avoid one more dependency. Decide when M1/M2 actually needs the tree.
- Final binary/crate name (`ttree-tile` is a placeholder).
- The exact keymap.
- Whether `ttree` gains a way to jump into `ttree-tile` directly (e.g. a keybind that
  execs it against the same session) versus the two being launched independently.
- VT-parser choice if `vt100` proves too limited on scrollback (evaluate `avt` or
  `wezterm-term` - noted as risk 4 in the exploration doc).
