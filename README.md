# ttree

![ttree: a Rust TUI for tmux sessions, showing the session tree and a live pane preview](docs/screenshot.png)

**Homepage:** [ttree.tek.cat](https://ttree.tek.cat/)

A terminal UI for tmux, written in Rust on [Ratatui](https://ratatui.rs). It renders your sessions, windows, and panes as a navigable tree, and embeds a **live preview** of any pane in the right-hand panel. The preview is the point: you can see what a pane is doing without attaching to it.

Built to solve a specific problem: driving many concurrent AI agent sessions running on a laptop from a phone, over Termux and a WireGuard VPN. Switching tmux contexts by hand to check on each one is slow and easy to get wrong. ttree puts every session in one tree and shows live output inline, so a glance replaces a context switch.

## Features

- Tree view of all sessions, windows, and panes, with smart collapsing
- Live pane preview: a real embedded PTY rendered in the right-hand panel
- Click a session in the sidebar to switch the preview, without leaving preview mode
- Persistent UI state (selection, expanded nodes, sidebar width) across restarts, stored in `~/.config/ttree/state.toml`
- Full mouse support: click to select, drag the separator to resize, scroll to navigate
- Mouse in the preview is passed straight through to tmux, so clicking a pane, dragging to select and scrolling the scrollback behave exactly as they do in your own tmux. ttree turns tmux's `mouse` option on for the session it is mirroring and puts your previous setting back when it stops
- Copies made in the preview are relayed onward as OSC 52, which works over SSH and through tmux
- Fuzzy filter (`/`) over the whole tree, keeping the parents of anything that matches so the tree stays a tree
- Manage tmux without leaving the TUI: create sessions and windows, split panes, rename, and kill anything (kills ask first and name what they will destroy)
- Vim-style navigation: `j` / `k` / `h` / `l`
- The preview is mirrored through a session grouped with the target, so browsing the tree never moves the window your other terminals are looking at
- Works on any tmux socket, including `tmux -L name` and `tmux -S /path`
- Real-time sync with tmux every 200ms, in a single tmux invocation per tick

## Stack

- **Rust 2021** + Cargo
- [ratatui](https://ratatui.rs): TUI framework
- [tokio](https://tokio.rs): async runtime
- [crossterm](https://docs.rs/crossterm): terminal event handling
- [portable-pty](https://docs.rs/portable-pty): PTY spawning for the live preview
- [vt100](https://docs.rs/vt100) + [tui-term](https://docs.rs/tui-term): VT100 parsing and rendering
- [indexmap](https://docs.rs/indexmap): ordered session, window, and pane collections

## Install

```bash
git clone git@gitlab.com:tek.cat/ttree.git
cd ttree
cargo install --path .
```

The binary lands in `~/.cargo/bin/ttree`. Requires Rust and an active tmux server.

Prefer a prebuilt binary? Every tagged release publishes stripped binaries for **x86_64 Linux (glibc)** and **aarch64 Android/Termux** on the [Releases page](https://gitlab.com/tek.cat/ttree/-/releases), built by CI.

## Run on Android / Termux

ttree is meant to be driven from a phone, so it runs natively on Android under Termux (`aarch64`). Building the full dependency tree on a phone tends to run out of RAM or disk, so the supported path is a cross-build on a laptop.

Two ways to get the binary onto the phone:

- **Prebuilt release binary (no toolchain needed).** Every tagged release attaches a stripped Android binary on the [Releases page](https://gitlab.com/tek.cat/ttree/-/releases). Pull the latest straight to the phone:

  ```sh
  curl -sSLo "$PREFIX/bin/ttree" \
    "https://gitlab.com/tek.cat/ttree/-/releases/permalink/latest/downloads/ttree-aarch64-linux-android"
  chmod +x "$PREFIX/bin/ttree"
  ```

- **Build it yourself.** No special dependency handling is needed: the tree builds for `aarch64-linux-android` as-is (no patches, no vendored crates). Cross-build on a laptop with `rustup` + Android NDK r28, then copy the binary over:

  ```sh
  scripts/build-android.sh                      # -> target/aarch64-linux-android/release/ttree
  ```

  Building natively inside Termux (`cargo build --release`) works too, but it is RAM- and disk-hungry on smaller phones, so the prebuilt binary above is usually the easier path.

In Termux, the install prefix is `$PREFIX/bin` (`/data/data/com.termux/files/usr/bin`). You also need `tmux` on the phone (`pkg install tmux`).

**Low-memory caveat:** ttree spawns a PTY + vt100 parser per visible pane, so on a memory-starved phone, pointing it at a busy tmux server can trip Android's low-memory killer (it can even take down `sshd`). Free RAM first, keep the visible pane count modest, and prefer `mosh` over plain SSH so the session survives a reconnect.

Full rationale, toolchain details, and the approaches that don't work (notably static musl): [`docs/android-termux-build.md`](docs/android-termux-build.md).

## Usage

Run `ttree` from any terminal, inside or outside tmux.

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
| `c` | Create window in the selected session |
| `%` / `"` | Split the selected pane right / below |
| `r` | Rename the selected session, or window |
| `x` | Kill the selected session, window or pane (asks first) |
| `/` | Filter the tree; `Esc` clears it |
| `Ctrl+P` | Toggle focus between tree and preview |
| `?` | Help |
| `q` / `Ctrl+C` | Quit |

In preview mode your tmux prefix (`Ctrl+B` unless you have changed it, ttree reads the `prefix` option) is reserved: `prefix d` returns to the tree, `prefix <key>` sends the prefix and key on to tmux, and pressing the prefix twice sends one copy through, which is how you reach a tmux or a ttree nested further in.

### tmux options ttree changes

While it is mirroring a session, ttree sets `mouse on` and `set-clipboard on` on that session, and puts your previous values back when it stops (including if it is killed). The first makes mouse passthrough do anything at all; the second is what lets a copy made in the preview reach your system clipboard, since tmux drops a relayed OSC 52 at its default `external`.

## Background

ttree began as a Rust rewrite of an earlier personal Python/[Textual](https://textual.textualize.io) prototype for the same workflow. This repository contains the Rust tool only.

## License

MIT. See [LICENSE](LICENSE).
