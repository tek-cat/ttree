# Exploration: ttree as an i3-style tiling manager for terminal sessions

Date: 2026-09-10
Status: research / spike findings. No code was written. This is a recommendation, not a spec.

## The idea

Turn ttree from a read-only tmux browser into an i3wm-style tiling manager: arbitrarily
nested containers (splits, tabbed groups, stacked groups), mouse drag-and-drop to
re-parent panes, and animated layout transitions rendered in ratatui. Separately: decide
whether tmux is the right engine underneath, or whether a modern tmux-like server is a
better foundation.

"cc messages" is read here as tmux **control mode** (`tmux -C` / `-CC`), the
machine-readable client protocol iTerm2 uses. If it meant Claude Code messaging, the
engine analysis still holds; only the input path changes.

## What "i3-like infinite nesting" actually requires

i3's model: every node is a **container** with a layout of `splith`, `splitv`, `tabbed`,
or `stacking`. Leaves are windows (for us: PTYs). Containers nest without limit, so a
tabbed container can hold a split that holds a stacked group. Core operations: split
(insert a parent container), change a container's layout, move a leaf in a direction
(re-parent it anywhere in the tree), focus-parent, and weighted resize of siblings.

tmux's model: `session -> window -> pane`. Panes inside a window *can* nest as arbitrary
h/v splits, so `splith`/`splitv` nesting is native. But:

- **No tabbed or stacking containers.** Windows are the only "tab", they are flat (one
  level, no nesting), and every pane in a window is visible at once.
- **No "move in a direction across the tree".** You get `swap-pane`, `join-pane`,
  `break-pane`. Re-parenting is possible but clumsy.

Conclusion: to get i3's real model you **build the tree yourself** and use whatever
engine sits underneath purely as a "PTY farm + persistence layer". No off-the-shelf
multiplexer gives you infinite tabbed/stacked nesting.

## Engine survey

| Engine | Language / license | Nesting model | Programmatic surface | Persistence | Fit |
|---|---|---|---|---|---|
| **tmux control mode** | C / ISC | h/v splits nest; windows flat; no tabs/stacks | `tmux -C`: send commands on stdin, receive `%output` / `%layout-change` / `%window-*` notifications for **every pane in every window of the attached session at once** | Server survives; full detach/resurrect | **Best.** Stream N windows, composite them yourself |
| **Zellij** | Rust / MIT (server is Apache-2.0; check before vendoring) | tiled + floating + **stacked** panes, flat tabs. "Stacked panes cannot have children" - still no infinite nesting | `zellij action`, `zellij pipe`, WASM plugin host commands (`new_tabs_with_layout`, `run_action`), community `zjctl`; a websocket **web client** (2025, undocumented) | Built-in session resurrection | Good for "nicer Zellij + mouse + animation"; wrong for genuine i3 nesting. Scripting surface, not a rendering surface |
| **wezterm mux** | Rust / MIT | pane tree per tab; tabs flat | `codec` crate: LEB128-framed PDUs, RPC + unilateral notifications, delta rendering. `mux` + `wezterm-term` crates are reusable | `wezterm-mux-server` daemon | Not a target to drive, but the **best-engineered Rust reference** for a detachable protocol and a terminal grid model |
| **3mux** | Go / MIT | **i3's model** (the only one that has it) | none - it is an end-user app | none (no detach) | Last release 2021-03; only sporadic community commits since (last commit 2026-07 was a typo fix); 39 open issues. **Read its tree + keybinding code as prior art; do not build on it** |
| **dvtm** + **abduco** | C / MIT | preset layouts only (tiling, grid, bstack, fullscreen) | none | via abduco | Reference for "smallest possible VT + splitter" |
| **mtm** | C / MIT | one h/v split tree | none | none | ~1k-line reference implementation |
| **shpool** | Rust / Apache-2.0 | none (no layout) | none | persistence only | Not relevant - pure session persistence |

### tmux control mode: the key facts

- One control client receives **all output for all panes in all windows** of the attached
  session, not just the visible window. This is what makes compositing multiple tmux
  windows into one ttree screen possible.
- `%output %<paneid> <data>` with bytes `<32` and `\` escaped as octal. `%extended-output`
  adds pane age for flow control. Flow control via `refresh-client -f pause-after=N` plus
  `%pause`/`%continue`.
- Command results are wrapped in `%begin`/`%end` (or `%error`) with a command serial.
- `%layout-change`, `%window-add`, `%window-close`, `%window-renamed`,
  `%session-changed`, `%unlinked-window-add`, etc. give you the topology.
- Output from tmux's own UI (copy-mode, choose-tree) is **not** sent.

## Recommended architecture: control-mode client on tmux, ttree owns the whole tree

**One tmux window per leaf terminal. tmux never splits anything. ttree's layout engine is
the only layout.**

```
                 ttree process
   +-----------------------------------------------+
   |  i3 tree:  Container{tabbed}                   |
   |              |- Container{splitv}              |
   |              |    |- leaf @4   (rect 0,0 80x20)|
   |              |    '- leaf @7   (rect 0,20 80x20)
   |              '- leaf @9        (rect ...)      |
   |                                               |
   |  layout engine -> rect per leaf               |
   |  per-leaf vt100 parser  <- %output bytes      |
   |  tui-term render into rect + own chrome        |
   |  tachyonfx effects layer                       |
   +----------------------|------------------------+
                          | stdin: commands
                          | stdout: %output / %layout-change / ...
                 +--------v---------+
                 |  tmux -C attach  |   (control client)
                 +--------|---------+
                          |
                 +--------v--------------------------------+
                 | tmux server                             |
                 |  window @4 [1 pane, size forced 80x20]  |
                 |  window @7 [1 pane, size forced 80x20]  |
                 |  window @9 [1 pane, size forced ...]    |
                 +----------------------------------------+
```

Flow:

1. ttree starts a `tmux -C` (or `-CC`) control client against a dedicated session.
2. ttree sets `window-size manual` on that session (mandatory - otherwise tmux shrinks
   every window to the smallest client). It saves and restores the previous value, the
   same pattern ttree already uses for `mouse` and `set-clipboard`.
3. For each leaf in ttree's tree, there is exactly one tmux window holding one pane.
   ttree computes the rect from its layout engine and issues
   `resize-window -t @id -x W -y H` so the pane is exactly the size of the rect it will
   be drawn into.
4. ttree feeds each window's `%output` bytes into a per-window VT parser and renders the
   grid into its rect with tui-term, then draws its own borders, tab bars, stack headers,
   focus ring, and animations on top.
5. Input goes to the focused leaf via `send-keys -H` (hex) / `send-keys -l` (literal);
   ttree already has a key-encoding layer. Mouse via `send-keys -M`.
6. Detach = quit ttree. Reattach = reconnect the control client, `capture-pane -pe` each
   window to seed the parsers, resume streaming.

Why this one:

- **Keeps ttree's reason for being.** Phone-driven over WireGuard + SSH, survives
  disconnects, other plain tmux clients can still attach to any individual window. tmux
  stays the server; none of that has to be rebuilt.
- **Delivers real i3 nesting.** The tree is 100% ttree's. tmux contributes PTYs and
  persistence and nothing else, so tabbed/stacked/split nest without limit.
- **Incremental.** ttree already spawns an embedded tmux client and runs a vt100 parser.
  Milestone 1 is "two windows side by side from one control client" - a contained proof.
  Then N. Then containers. Then tabbed/stacked. Then drag-drop. Then effects.
- **No lock-in.** The layout engine, the compositor, the input router, and the effects
  layer are all engine-agnostic. If control-mode latency or throughput disappoints, that
  code repoints at a private `portable-pty` farm (architecture C below) without a
  rewrite. **The tree engine is the asset; the backend is swappable.**

### Rejected alternatives

- **Drive Zellij.** You inherit Zellij's layout ceiling (splits + stacks + floats, flat
  tabs), so you would be building a fancy remote control, not a new tiling model. Keep
  Zellij as proof that a Rust TUI multiplexer with plugins and a web client is viable,
  and as a source of UX ideas (status hints, swap layouts, floating panes).

- **Fork Zellij.** See the next section - the fork ratio is bad. The parts you would
  fork it for are fine as-is; the parts you want are exactly the parts you would gut.
- **ttree becomes its own multiplexer now (architecture C).** Own daemon, own PTYs, own
  detach protocol, own copy-mode, own session resurrection, own scrollback persistence,
  Unicode width, image passthrough. This is the Zellij / wezterm-mux scope - months of
  work and a permanent maintenance surface. Right destination *if* control mode proves
  inadequate, not the starting point. If it happens, vendor wezterm's `codec` +
  `wezterm-term` crates rather than starting from zero.

## Forking Zellij

Zellij is MIT-licensed, so a fork is legally clean. The problem is the fork *ratio*.

**What Zellij is, internally:**

- Client-server over a Unix socket, bincode messages, multiplayer, session
  resurrection. The server owns panes / tabs / plugins / PTYs; the client is thin -
  it writes bytes and forwards input.
- Its **own** rendering pipeline: a `vte`-based parser feeds a per-pane `Grid` (viewport
  + scrollback + cursor + styles); the server composites all grids into an `OutputBuffer`,
  diffs it, and streams ANSI to the client. **No ratatui anywhere.**
- Tiled layout is a **constraint grid**, not a free-form i3 tree: each pane carries a
  `PaneGeom` with `Dimension`s that are `Fixed(n)` or `Percent(f)`, and `TiledPaneGrid`
  solves the constraints so edges line up. Runtime restructuring is limited to what the
  actions expose; the deep KDL nesting is mostly an *initial-layout* feature. Tabs are
  top-level only, stacked panes cannot have children, floating panes are a separate
  absolute-coordinate collection.
- A WASM plugin runtime (migrated wasmtime -> wasmi), a KDL config system, a theme spec,
  a web client (2025). Dozens of crates, well into five figures of Rust.

**The ratio problem:**

| You would fork Zellij *for*... | ...but that part is already fine |
|---|---|
| client-server + detach + session resurrection | yes - and it is the hardest infra to build |
| mature VT parser + grid + scrollback + copy-mode + search | yes |
| mouse, OSC 52, clipboard, SGR | yes (ttree already has the OSC 52 half) |

| You actually *want*... | ...and that is exactly what you must gut |
|---|---|
| true i3 infinite nesting (tabbed in split in stacked, no limit) | rewrite `TiledPaneGrid` / the whole layout engine |
| ratatui + tachyonfx animation and drag-drop | replace the ANSI compositor with a ratatui render path |

When you are replacing the layout engine **and** the renderer, you are not forking - you
are doing a ground-up rewrite while first having to understand tens of thousands of lines
of someone else's Rust and then tracking or abandoning an active upstream. That is
strictly more work than "own multiplexer, vendor `wezterm-term` for the grid" (architecture
C), which starts from a clean sheet.

**When forking Zellij would actually make sense:**

1. **The i3 vision is negotiable.** If "Zellij, but with mouse drag-drop and some
   transition polish" is acceptable - keeping its splits+stacks+floats+flat-tabs model -
   then do not even fork: write a **plugin**, or contribute drag-drop upstream. Zellij's
   maintainers are active and have been steadily adding layout power (stacked panes,
   stacked resize in 0.42).
2. **You want the batteries** - multiplayer, web client, plugin ecosystem, resurrection -
   more than you want ratatui, and you are willing to build the i3 tree as an alternative
   pane-manager *inside* `zellij-server`. Possible, but you are fighting the codebase the
   whole way, and the renderer still is not ratatui.
3. **Upstream collaboration**, not a fork: propose "true nested containers" as a new
   layout mode. Uncertain timeline, but zero maintenance burden and the feature lands for
   everyone.

**Verdict:** forking Zellij is the worst of the options for *this* goal. It only wins if
the ratatui/animation requirement is dropped, and in that case the right move is a plugin
or an upstream contribution, not a fork.

## Mouse drag-and-drop

Feasible with bounded work. ttree already has SGR mouse encoding, a separator-drag
handler, and sidebar hit-testing.

- On drag start over a leaf, capture that leaf's frozen cell buffer as the drag "ghost".
- While dragging, hit-test the leaf under the cursor and pick a drop zone: outer
  third left/right/top/bottom = split in that direction; center = add to the target's
  tab/stack group (creating a `tabbed` container if the target is a bare leaf).
- Render a translucent drop-zone overlay (a tachyonfx pulse on the target rect).
- On drop, mutate the tree, recompute rects, animate to the new layout, then send the
  real `resize-window` calls once the animation settles.

The one hard part is hit-testing an arbitrarily nested tree against a cursor cell, which
is a standard recursive rect walk.

## Animation

Use **tachyonfx** (now an official `ratatui/tachyonfx` crate: 50+ cell-level effects -
`slide_in`/`slide_out`, `dissolve`, `coalesce`, HSL transitions, spatial patterns,
operating on cells after widgets render). Natural uses: tab switch = slide, pane close =
dissolve, pane open = coalesce, focus change = brief highlight sweep, drop preview =
pulse.

**The real constraint is the PTY, not the drawing.** Terminal content is a grid at a
fixed cell size. Animating a split 50% -> 60% would mean resizing both child PTYs every
frame; each SIGWINCH makes the program inside reflow and repaint - ugly and expensive
mid-animation, and sub-cell smoothness is impossible in a TUI regardless.

**So animate the chrome, not the content.** During a resize or move:

1. Freeze each affected pane's last rendered grid as a static cell buffer.
2. Slide / scale *those frozen buffers* (nearest-neighbour) across the animation frames
   with tachyonfx.
3. When the animation settles, send the real `resize-window` and let each pane repaint
   exactly once.

This is how tiling-WM animations that feel right actually work (PaperWM, Hyprland animate
the frozen surface, then reflow). You get per-cell steps, not sub-pixel, but an 8-cell
slide over ~150ms at a 60fps redraw reads as smooth. i3 itself has no animation at all,
so this is already ahead.

Needs a per-pane "frozen buffer" abstraction: snapshot `Vec<Cell>` + rect, renderable
independently of the live parser.

## Risks to retire before committing (cheap spikes)

1. **Independent window sizes.** Confirm `window-size manual` + per-window
   `resize-window` actually holds different sizes for different windows under a single
   control client on current tmux (3.4 / 3.5). iTerm2 had bugs here historically
   ("both windows change size"). ~30 min.
2. **Input latency.** Measure round-trip of `send-keys -l` through control mode vs a
   direct PTY for the focused pane. If it is bad, design a hybrid: the focused leaf gets
   a real `tmux attach` PTY for zero-latency typing, the rest get control-mode streaming.
3. **Output throughput.** Run `yes` in 10 panes at once. Does `%output` keep up? Does
   `refresh-client -f pause-after` / `%pause` behave? Does the parser CPU hold on a
   phone?
4. **Scrollback ownership.** ttree's own scrollback needs more than vt100 0.16 gives
   comfortably - evaluate `avt` (asciinema's VT crate) or `wezterm-term` - versus
   delegating scrollback to tmux copy-mode per pane on demand.
5. **Window-list pollution.** One-window-per-leaf makes the session's window list long
   for anyone attaching with plain tmux. Use a dedicated session name, or accept it.

## Open product questions

- **One binary or two?** A `--tile` mode on `ttree`, a sibling binary, or a new project.
  The tree browser and the tiling manager are different enough tools that a sibling
  binary sharing a workspace crate is probably cleanest, but that is a call to make
  up front.
- Does the tree browser stay as-is and the tiling view become a second mode reachable
  from it, or does tiling replace the preview pane entirely?
- Config format for saved layouts (KDL like Zellij, TOML like ttree's current state,
  or the i3 tree serialized as JSON).

## Prior art worth reading

- **3mux** source (`aaronjanse/3mux`): the tree model and i3 keybinding semantics.
- **wezterm** `codec` / `mux` / `wezterm-term` crates: detachable protocol and terminal
  grid model in Rust.
- **Zellij** `zellij-server` compositor and the KDL layout schema (for saved-layout
  design; mind the license before copying code).
- **dvtm**: minimal VT + splitter in C.
- tmux wiki **Control-Mode** page and iTerm2's tmux integration notes for the
  control-mode edge cases.

## Sources

- tmux Control Mode: https://github.com/tmux/tmux/wiki/Control-Mode
- tmux(1) manual: https://man7.org/linux/man-pages/man1/tmux.1.html
- tmux `window-size` / `resize-window`: https://github.com/tmux/tmux/issues/2594 , https://github.com/tmux/tmux/issues/1637
- iTerm2 variable window size bug history: https://gitlab.com/gnachman/iterm2/-/issues/7970
- Zellij programmatic control: https://zellij.dev/documentation/programmatic-control.html , https://zellij.dev/documentation/plugin-api-commands.html
- Zellij stacked panes / nesting limit: https://zellij.dev/features/ , https://github.com/zellij-org/zellij/issues/4673
- Zellij rendering pipeline / Grid / vte / OutputBuffer: https://deepwiki.com/zellij-org/zellij , https://poor.dev/blog/performance/
- Zellij license (MIT): https://github.com/zellij-org/zellij/blob/main/LICENSE.md
- Zellij wasmtime -> wasmi plugin runtime: https://zellij.dev/news/
- Zellij 0.42 stacked resize / pinned panes: https://zellij.dev/news/stacked-resize-pinned-panes/
- zjctl: https://github.com/mrshu/zjctl
- 3mux: https://github.com/aaronjanse/3mux , https://www.linuxlinks.com/3mux-terminal-multiplexer-i3/
- wezterm multiplexing: https://wezterm.org/multiplexing.html , https://deepwiki.com/wezterm/wezterm/2.2.2-client-server-protocol
- tachyonfx: https://github.com/ratatui/tachyonfx , https://ratatui.rs/ecosystem/tachyonfx/
- awesome-terminal-multiplexers: https://github.com/arran4/awesome-terminal-multiplexers
