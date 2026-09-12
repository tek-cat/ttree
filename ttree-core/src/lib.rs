//! Shared plumbing for ttree's binaries: tmux read/write commands, the
//! tmux-status-bar theme, config-directory resolution, and the terminal
//! input-encoding layer. See `docs/superpowers/specs/2026-09-12-i3-tiling-design.md`
//! for why this crate exists.

pub mod actions;
pub mod tmux_client;
