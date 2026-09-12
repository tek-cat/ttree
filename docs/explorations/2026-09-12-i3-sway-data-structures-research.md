# Research: i3 and sway's tree data structures, for ttree-tile's performance requirement

Date: 2026-09-12
Status: research findings (two parallel subagents, grounded in cloned source). Feeds the
"Core data model" section of `docs/superpowers/specs/2026-09-12-i3-tiling-design.md`.
Requirement this serves: [[project_ttree_tiling_performance]] - the tiling direction must
be "super performant".

Two agents independently researched i3 (X11, `github.com/i3/i3`) and sway (Wayland,
`github.com/swaywm/sway` + `wlroots`), each grounded in a shallow clone of the actual
source with exact file/line citations. Full reports below, verbatim. The synthesis is in
the spec doc; this file is the raw material.

## i3: `Con`, TAILQ, percent-based sizing, focus bubbling

Grounded in `/tmp/i3-research` (shallow clone, default branch).

### 1. Shape of the container tree

Everything is one struct: `Con` (`include/data.h:659-825`). No separate workspace/split
types - `Con.type` (`CT_ROOT, CT_OUTPUT, CT_CON, CT_FLOATING_CON, CT_WORKSPACE,
CT_DOCKAREA`) and `Con.layout` (`L_DEFAULT/STACKED/TABBED/DOCKAREA/OUTPUT/SPLITV/SPLITH`)
discriminate role. Root, outputs, workspaces, split containers, and windows live in the
same tree at different depths, uniformly.

Children are intrusive doubly-linked lists (`TAILQ_HEAD`/`TAILQ_ENTRY` from BSD
`sys/queue.h`), not arrays. Each `Con` embeds five list heads (`nodes_head` - layout
order, `focus_head` - focus/MRU order, `floating_head`, `swallow_head`, `marks_head`) and
four link entries (`nodes`, `focused`, `all_cons`, `floating_windows`), so one `Con` is a
member of several independent orderings at once. This gives O(1) insert-at-head/tail,
insert-before/after, and remove-given-pointer, with no shifting or reallocation. i3's own
devguide: *"Usually, TAILQ is used which allows inserting elements at arbitrary positions
or at the end of the list"* (`docs/hacking-howto:328-330`).

Parent linkage is a single raw pointer (`data.h:694`). Every `Con` is individually
heap-allocated (`scalloc`, `src/con.c:39`) and manually freed (`con_free`,
`src/con.c:80-97`) - no arena, no generational IDs, just raw pointers. This is exactly
the part a Rust rewrite should not imitate (see "what not to copy" below).

Orientation is *not* a stored field - it's derived on demand from `layout` via
`con_orientation()` (`src/con.c:1620-1651`). Avoids "layout and orientation disagree"
bugs. Directly applicable: derive orientation from the split/tab/stack enum, don't
duplicate it as separate state.

Sizing is proportional, not absolute: `Con.percent` (a `double`) is this child's share of
the parent's space along the split axis; `con_fix_percent()` renormalizes all siblings to
sum to 1.0 on attach/detach. `Con.rect`/`window_rect`/`deco_rect` are explicitly
documented as ephemeral: *"these properties... are temporary, meaning they will be
overwritten by calling render_con. Persistent position/size information is kept in
[percent]"* (`docs/hacking-howto:483-484`). **The single most important structural
decision to carry over**: persist only the proportional split ratio; recompute rects from
the current viewport every frame.

### 2. Resize/re-layout: full top-down recompute, by design

`tree_render()` does a full top-down recompute of every rect on every call, and i3's own
devguide says so outright: *"re-rendering everything is not actually necessary... Focus
was on getting it working correct, not getting it work very fast"*
(`docs/hacking-howto:487-491`). What's actually incremental is the *mutation*: a resize
only touches two adjacent siblings' `percent` fields plus a renormalization pass over
direct siblings - never rects or descendants directly. The subsequent full-tree render
pass is cheap because it's pure in-memory arithmetic over a small tree (`render_con` is
explicitly "side-effect free... completely done in memory only",
`src/render.c:34-38`).

The real optimization is where it's expensive: talking to X11. `x_push_node()` keeps a
shadow cache (`con_state`, `src/x.c:38-71`) and diffs the freshly-rendered rect against
the cached one before issuing any `xcb_configure_window` call. Decoration redraw is
similarly cached, saving (per the devs) "far over 50%" of redraws.

**Pattern to carry over**: cheap full in-memory relayout every time, diffed/cached push to
whatever's actually expensive (for ttree-tile: tmux `resize-window` calls and terminal
writes, not the ratatui geometry math).

### 3. Focus tracked separately from tree shape

Each `Con` has its own `focus_head`: the same children as `nodes_head`, reordered by
recency of focus, independently at every level. Plus one global `Con *focused` pointer
for O(1) "what's active". `con_focus()` moves the container to the head of its parent's
focus list, then recurses to do the same in the parent's own parent - bubbling all the way
to the root. Consequences: finding the focused leaf from any subtree is O(depth), not
O(n) (`con_descend_focused`); picking what to focus next after closing something is also
O(depth) (`con_next_focused`); stacked/tabbed rendering order reuses the same list so the
focused child paints on top.

**The cleanest transferable idea in the whole codebase**: two orderings over the identical
child set (layout-order, MRU-order), each essentially free to maintain, plus one O(1)
global cursor to the active leaf.

### 4. Explicitly performance-motivated elements

- Single X11 flush per event-loop iteration (`ev_prepare` hook), not per mutation -
  coalesces many buffered changes into one syscall-level write.
- Shadow-state diff before touching the backend (`con_state`, as above).
- **`tree_flatten()`**: collapses a container with exactly one child whose orientation
  matches its own parent's, re-attaching grandchildren directly and closing the redundant
  wrapper. Directly relevant to ttree-tile's "unlimited nesting": without an equivalent,
  repeated split/close cycles leave behind ever-deeper chains of pointless wrapper
  containers.
- Split-in-place shortcut: reuses the target's parent (just flips `layout`) instead of
  allocating a new wrapper container when the parent already has exactly one child.
- `percent`-based sizing avoids re-deriving ratios from pixels each frame, sidestepping
  rounding-error accumulation.

### 5. What is X11/C-specific - do not copy

- Raw pointers + manual alloc/free as the entire memory model. Use an arena/slotmap with
  typed `NodeId` indices in Rust instead - same O(1) ergonomics, no aliasing/use-after-free
  risk.
- Unbounded native-stack recursion with no depth guard (fine for i3's typical nesting
  depth; not fine given ttree-tile's explicit "unlimited nesting" goal - walk iteratively
  or bound depth).
- O(n) linear scans for ID lookup (`con_by_con_id` etc., `TAILQ_FOREACH` over a flat
  list) - a real, acknowledged i3 inefficiency, tolerated only because window counts are
  tiny. Use a `HashMap<Id, NodeId>` instead; costs nothing extra to maintain.
- X11-specific fields with no ratatui/tmux analogue (pixmap/frame buffers, depth/colormap,
  urgency timers, unmap-race counters).
- The specific `con_state`/CIRCLEQ diff cache data is X11-protocol-shaped; the *pattern*
  (cache last-pushed state, diff before acting) is what's worth keeping.
- EWMH-flavored fullscreen/floating semantics are X11 window-manager protocol concerns.

## sway + wlroots: array-based tree, pending/current split, damage tracking

Grounded in `/tmp/sway-research` (`swaywm/sway` @ `793a1f0`) and `/tmp/wlroots-research`
(`wlroots` @ `4105e70`).

### 1. Array-based tree (`list_t`), not i3's intrusive TAILQ

`struct sway_node` (`include/sway/tree/node.h`) is a tagged union
(`N_ROOT/N_OUTPUT/N_WORKSPACE/N_CONTAINER`) with a `bool dirty` flag. Every
container-ish level stores children as `list_t *` - a plain growable array of `void*`
(`common/list.c`: starts at capacity 10, doubles via `realloc`). No design-rationale
commit was found explaining the choice versus i3's TAILQ; sway's `list.h` appears to be
original utility code, not something migrated from i3.

**What the array costs, evidenced in the code**: `container_detach`/`container_add_sibling`
do `list_find` (O(n) linear scan) + `list_del`/`list_insert` (a `memmove` shifting
elements) - a real asymptotic regression versus i3's O(1) TAILQ surgery, irrelevant in
practice at sway's realistic child counts (single digits to low tens).

**What the array buys**: dense, cache-friendly iteration, which is exactly what
layout/arrange code does every time (walk all children, compute fractions and
positions). An array is the better shape for "iterate everything and recompute"
workloads; an intrusive list is better for "insert/remove by pointer without knowing
position." Sway's actual mutation pattern (rare structural edits, frequent full-subtree
layout recompute) favors the array.

**Sway is not dogmatically array-only** - it picks the structure per access pattern.
Focus order (needs O(1) move-to-front, needs one node trackable across multiple
independent per-input-seat orderings at once) is *not* forced into `list_t`. Instead each
seat wraps a focused node in `struct sway_seat_node` threaded through an intrusive
`wl_list focus_stack` per seat - wlroots' own generic intrusive list, i3's TAILQ's
cousin, used exactly where i3 would use one. **Lesson: use the right structure per
access pattern, not one structure everywhere** - array for the containment tree
(bulk-iterated, rarely mutated), intrusive/indexed list for focus order (frequently
reinserted at head).

At the rendering layer (wlroots' scene graph, separate from sway's logical tree),
children are intrusive again (`wl_list` in `wlr_scene_node`/`wlr_scene_tree`) because
z-order restacking happens on every focus change and reparent happens on every layout
pass - both O(1) list-splice operations.

### 2. Double-buffered state + a flat dirty list (the load-bearing mechanism)

Every stateful node keeps two copies of its state: `pending` (what layout code writes)
and `current` (what's committed and safe to read for rendering). The flow:

1. **Mutation** writes into `pending` and calls `node_set_dirty()`, which is a guarded,
   idempotent append: *"if (node->dirty || node->destroying) return; node->dirty = true;
   list_add(server.dirty_nodes, node);"* - one flat global list, no tree walk needed to
   find what's dirty, and a node already queued is never re-added.
2. **Pure layout math over `pending`**: `arrange.c`'s layout functions recompute
   `pending.{x,y,width,height}` from normalized fractions, and recurse
   **unconditionally into every child** - there is no early-exit for unchanged subtrees.
   Sway does not try to make this pass incremental; it relies on cheap array iteration
   plus the two-phase commit to absorb the cost.
3. **Transaction commit** copies `pending` to `current` (memcpy) for every dirty node in
   one pass, batching an arbitrary number of scattered mutations from one input event into
   one atomic step, then clears the dirty list.
4. **Second pass pushes `current` into the scene graph** via setters that each
   self-guard on equality (see below) - so this second full walk is cheap: a walk of
   comparisons, with actual expensive redraw work firing only where something differs.

**Takeaway**: the pending/current split solves *Wayland's* cross-process atomicity
problem (a resize must wait for another process's client to redraw in lockstep, or you
get tearing) - not needed when one process owns every pane's grid directly. But the
*pattern* - one global flat dirty list + full-but-cheap recompute of an already-narrowed
working set + a final apply step whose individual setters self-guard on equality - is
exactly right to adapt, minus the transaction/serial/timeout machinery.

### 3. wlroots damage tracking

Every scene node maintains a `visible` region (screen-space rects where it's the topmost
uncovered thing). Every mutator guards on no-op first (`if (node->x == x && node->y ==
y) return;`) before computing anything. When something does change, the update walks
only nodes whose bounds intersect the changed region's bounding box (spatially pruned,
not a full-tree walk), unions the result into a per-output damage ring, and the whole
output-commit pipeline short-circuits entirely (zero further work, no syscalls) when that
ring is empty - checked at both the wlroots level (`wlr_scene_output_needs_frame`) and
sway's own repaint-timer handler.

The damage ring caps rectangle count (`WLR_DAMAGE_RING_MAX_RECTS = 20`) and collapses to
a bounding box past that threshold - a cheap, portable defensive bound: trade precision
for a bounded per-frame cost rather than let a diff list grow unbounded.

### 4. Concrete techniques worth adapting (portable, not GPU/Wayland-specific)

1. Equality-guard every mutator before marking anything dirty (`if new == old { return;
   }`) - the single cheapest win.
2. One flat global dirty list with an idempotent-append guard, not per-node bookkeeping.
3. Separate "recompute logical layout" (cheap, full, arithmetic-only) from "apply to the
   render target" (where equality checks decide what's actually expensive to redo).
4. Bounding-box-pruned traversal: skip recursing into a subtree whose computed rect
   didn't change versus its cached value.
5. Accumulate damage/dirty regions across frames rather than recomputing from scratch;
   cap the tracked count and fall back to "whole thing dirty" past the cap.
6. Gate the entire render+flush pipeline behind a single "anything dirty?" check, at both
   the per-pane and per-frame level - an idle compositor (or ttree-tile) should do zero
   work on a quiet tick.
7. Schedule redraws around actual events (PTY output, input, layout mutation), not a
   fixed poll.
8. Use the right structure per access pattern (array for bulk-iterated tree structure,
   indexed/intrusive structure for frequently-reordered focus/z-order lists) - don't
   force every kind of ordering through the same collection type.
9. Cache last-committed geometry per node so a resize only needs to touch the subtree
   whose ancestor chain's rect actually changed.
10. A `generation`/version counter per leaf, bumped on new content, compared against
    "last drawn" at render time, is a lighter-weight alternative to a flag-plus-list for
    deciding whether a pane's content needs re-blitting - pairs naturally with a VT
    parser that already tracks "did I just process new bytes."

### 5. What is wlroots/Wayland/GPU-specific - do not copy

- The transaction/serial/ack/timeout machinery exists solely because resizing a Wayland
  client in *another process* needs a round-trip and a timeout fallback to avoid tearing.
  A terminal tiling manager owns every pane's VT100 grid directly, in-process, on the
  same tick - no analogous race.
- Direct scanout (bypassing compositing to hand a buffer straight to the display
  controller) is GPU/DRM-specific with no terminal-cell equivalent.
- Pixman regions, sub-pixel/output scaling, coordinate transforms for rotated/scaled
  displays - meaningless for an integer character grid.
- Buffer/swapchain management and the damage ring's multi-buffer rotation exist because
  GPU compositors juggle 2-3 physical framebuffers; a terminal has one buffer the process
  controls synchronously.
- Occlusion-based visibility (subtracting opaque regions) matters when surfaces can
  overlap; in a strictly tiling layout (both sway's tiled containers and ttree-tile's
  target) siblings never overlap by construction, so there's no occlusion to compute -
  drop this rather than port it, unless floating/overlapping panes get added later.

## Synthesis: what this changes in ttree-tile's data model

See the spec doc's "Core data model" section for the resulting Rust sketch. In short:

- **Arena + typed `NodeId`**, not raw pointers or nested `Box<Node>` - gets i3's O(1)
  attach/detach/focus-bump without the raw-pointer/manual-free/use-after-free liabilities.
- **`Vec<NodeId>` for display-order children** (sway's choice: cache-friendly, matches
  the "iterate everything every layout pass" access pattern) **plus a separate
  `focus_order: Vec<NodeId>`** (i3's choice: MRU-first, updated by remove+push-front,
  bubbled to the parent on every focus change) and **one `focused: NodeId` cursor** for
  O(1) "what's active".
- **Weights (`f32` per child), not rects, as the persisted truth**; rects are recomputed
  top-down from the current viewport every frame - cheap, because it's arithmetic over a
  small tree, exactly i3's position ("correct and simple beats incremental").
- **Prune degenerate single-child wrapper containers** on every detach (i3's
  `tree_flatten`), so "unlimited nesting" doesn't silently become "unbounded wrapper
  accumulation" in memory and traversal depth.
- **A flat, idempotent `dirty: Vec<NodeId>` list**, appended to by every mutation, drained
  once per render tick (sway's `server.dirty_nodes` + `node_set_dirty` guard) - decides
  whether the render loop has anything to do at all this tick.
- **Equality-guard before any tmux-facing call**: cache each leaf's last-pushed rect and
  skip `resize-window` when the newly computed rect matches it; this, not the ratatui
  geometry math, is where ttree-tile's actual cost lives.
- **A `generation: u64` counter per leaf**, bumped when its VT parser processes new
  bytes, compared against "last drawn" to skip re-blitting a pane whose content hasn't
  changed.
