use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableMouseCapture, Event, KeyCode, KeyModifiers, KeyboardEnhancementFlags, MouseButton,
        MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io,
    sync::{Arc, RwLock},
    time::Duration,
};

mod state;
mod theme;
mod tmux_client;
mod ui;

use crate::state::{AppState, EmbeddedTerminal, Panel};
use crate::tmux_client::Tmux;

fn setup_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            PopKeyboardEnhancementFlags,
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste
        );
        // Don't leave a previewed session's options pinned if we crash.
        restore_pins_blocking();
        default_hook(panic_info);
    }));
}

async fn sync_state(state: &mut AppState) -> Result<bool> {
    let (sessions_raw, windows_raw, panes_raw) =
        tokio::join!(Tmux::list_sessions(), Tmux::list_windows(), Tmux::list_panes());

    let sessions_raw = sessions_raw.unwrap_or_default();
    let windows_raw = windows_raw.unwrap_or_default();
    let panes_raw = panes_raw.unwrap_or_default();

    let old_panes: std::collections::HashSet<String> = state.panes.keys().cloned().collect();

    let mut expanded_ids: std::collections::HashSet<String> =
        state.sessions.iter().filter(|(_, s)| s.expanded).map(|(id, _)| id.clone()).collect();
    expanded_ids.extend(state.windows.iter().filter(|(_, w)| w.expanded).map(|(id, _)| id.clone()));

    let is_initial_load = state.sessions.is_empty();

    // If we just loaded from disk and have no sessions yet, use the pre-loaded expanded_ids
    if expanded_ids.is_empty() && !state.expanded_ids.is_empty() {
        expanded_ids = state.expanded_ids.clone();
    }

    state.sessions.clear();
    state.windows.clear();
    state.panes.clear();

    for s in sessions_raw {
        let parts: Vec<&str> = s.split('\u{001f}').collect();
        if parts.len() >= 4 {
            let id = parts[0].to_string();
            // Our own grouped mirror sessions are an implementation detail of
            // the preview, not something the user should have to look at or
            // navigate into.
            if parts[1].starts_with(MIRROR_SESSION_PREFIX) {
                continue;
            }
            let expanded = if is_initial_load && state.expanded_ids.is_empty() {
                true // Default to expanded on very first run
            } else {
                expanded_ids.contains(&id)
            };

            state.sessions.insert(
                id.clone(),
                crate::state::Session {
                    id: id.clone(),
                    name: parts[1].to_string(),
                    windows: Vec::new(),
                    expanded,
                    last_attached: parts[2].parse().unwrap_or(0),
                    attached: parts[3] == "1",
                },
            );
        }
    }

    for w in windows_raw {
        let parts: Vec<&str> = w.split('\u{001f}').collect();
        if parts.len() >= 6 {
            let id = parts[0].to_string();
            let session_id = parts[1].to_string();

            // A grouped session shares its windows, so `list-windows -a` returns
            // each of ours twice: once under the real session and once under our
            // mirror. Keeping the mirror's copy would overwrite the window's real
            // owner, and every lookup from a pane back to its session would then
            // answer with the mirror, which reads as the client having drifted
            // and rebuilds the mirror on every sync. Sessions are parsed first,
            // so anything whose session we dropped gets dropped here too.
            if !state.sessions.contains_key(&session_id) {
                continue;
            }

            let expanded = if is_initial_load && state.expanded_ids.is_empty() {
                true // Default to expanded on very first run
            } else {
                expanded_ids.contains(&id)
            };

            state.windows.insert(
                id.clone(),
                crate::state::Window {
                    id: id.clone(),
                    session_id: session_id.clone(),
                    name: parts[2].to_string(),
                    panes: Vec::new(),
                    active: parts[3] == "1",
                    width: parts[4].parse().unwrap_or(80),
                    height: parts[5].parse().unwrap_or(24),
                    expanded,
                },
            );
            if let Some(session) = state.sessions.get_mut(&session_id) {
                session.windows.push(id);
            }
        }
    }

    for p in panes_raw {
        let parts: Vec<&str> = p.split('\u{001f}').collect();
        if parts.len() >= 9 {
            let id = parts[0].to_string();
            let window_id = parts[1].to_string();

            // `list-panes -a` reports a shared pane once per session in the
            // group, so our own mirror doubles every pane. Inserting the second
            // copy pushes the pane into its window's list twice, and the tree
            // then holds two rows with the same id: moving down from the first
            // lands on the second, which looks exactly like the selection being
            // stuck. Keep the first sighting and ignore repeats.
            if state.panes.contains_key(&id) {
                continue;
            }
            let left = parts[5].parse().unwrap_or(0);
            let top = parts[6].parse().unwrap_or(0);
            let width = parts[7].parse().unwrap_or(0);
            let height = parts[8].parse().unwrap_or(0);

            state.panes.insert(
                id.clone(),
                crate::state::Pane {
                    id: id.clone(),
                    window_id: window_id.clone(),
                    title: parts[2].to_string(),
                    current_command: parts[3].to_string(),
                    active: parts[4] == "1",
                    region: Some(ratatui::layout::Rect::new(left, top, width, height)),
                },
            );
            if let Some(window) = state.windows.get_mut(&window_id) {
                window.panes.push(id);
            }
        }
    }

    let new_panes: std::collections::HashSet<String> = state.panes.keys().cloned().collect();
    let changed = old_panes != new_panes;

    let current_list = state.get_dynamic_visible_items();

    if let Some(sel) = &state.focus.selected_id {
        if !current_list.contains(sel) {
            state.focus.selected_id = None;
        }
    }

    if state.focus.selected_id.is_none() && !current_list.is_empty() {
        // Try to find the most appropriate active pane to focus
        let mut active_pane = if let Ok(current_pane) = std::env::var("TMUX_PANE") {
            state.panes.get(&current_pane).cloned()
        } else {
            None
        };

        // 2. Try the last target we attached to in this ttree session
        if active_pane.is_none() {
            if let Some(target_id) = &state.last_target_id {
                if let Some(p) = state.panes.get(target_id) {
                    active_pane = Some(p.clone());
                } else if let Some(w) = state.windows.get(target_id) {
                    active_pane = w
                        .panes
                        .iter()
                        .find_map(|pid| state.panes.get(pid).filter(|p| p.active))
                        .cloned();
                } else if let Some(s) = state.sessions.get(target_id) {
                    active_pane = s
                        .windows
                        .iter()
                        .find_map(|wid| {
                            state.windows.get(wid).filter(|w| w.active).and_then(|w| {
                                w.panes
                                    .iter()
                                    .find_map(|pid| state.panes.get(pid).filter(|p| p.active))
                            })
                        })
                        .cloned();
                }
            }
        }

        // 3. Fallback to sorting by last_attached
        if active_pane.is_none() {
            let mut active_panes: Vec<_> = state
                .panes
                .values()
                .filter(|p| {
                    p.active && state.windows.get(&p.window_id).map(|w| w.active).unwrap_or(false)
                })
                .collect();

            // Sort by session activity to pick the most recently active one
            active_panes.sort_by(|a, b| {
                let sess_a =
                    state.windows.get(&a.window_id).and_then(|w| state.sessions.get(&w.session_id));
                let sess_b =
                    state.windows.get(&b.window_id).and_then(|w| state.sessions.get(&w.session_id));

                let time_a = sess_a.map(|s| s.last_attached).unwrap_or(0);
                let time_b = sess_b.map(|s| s.last_attached).unwrap_or(0);

                if time_a != time_b {
                    return time_b.cmp(&time_a); // Newest first
                }

                // Tie-breaker: attached sessions
                let att_a = sess_a.map(|s| s.attached).unwrap_or(false);
                let att_b = sess_b.map(|s| s.attached).unwrap_or(false);
                att_b.cmp(&att_a)
            });

            active_pane = active_panes.first().cloned().cloned();
        }

        if let Some(pane) = active_pane {
            state.focus.selected_id = Some(pane.id.clone());
            state.focus.nav_mode = crate::state::NavMode::Pane;
            // Ensure path to active pane is expanded
            if let Some(window) = state.windows.get_mut(&pane.window_id) {
                window.expanded = true;
                if let Some(session) = state.sessions.get_mut(&window.session_id) {
                    session.expanded = true;
                }
            }
        } else {
            state.focus.selected_id = Some(current_list[0].clone());
        }
    }

    Ok(changed)
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_panic_hook();

    let mut state = AppState::load_from_disk();
    state.theme = theme::Theme::from_tmux().await;
    loop {
        match run_app(&mut state).await {
            Ok(()) => {
                let last_target = state.last_target_id.clone();
                // Reload state from disk to get the latest focus/expanded state
                state = AppState::load_from_disk();
                state.last_target_id = last_target;
                state.theme = theme::Theme::from_tmux().await;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
}

fn encode_mouse(
    mouse: &crossterm::event::MouseEvent,
    x_offset: u16,
    y_offset: u16,
) -> Option<Vec<u8>> {
    use crossterm::event::{MouseButton, MouseEventKind};

    let col = mouse.column.saturating_sub(x_offset) + 1;
    let row = mouse.row.saturating_sub(y_offset) + 1;

    let (base_button, release) = match mouse.kind {
        MouseEventKind::Down(btn) => (
            match btn {
                MouseButton::Left => 0u32,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
            },
            false,
        ),
        MouseEventKind::Up(btn) => (
            match btn {
                MouseButton::Left => 0u32,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
            },
            true,
        ),
        MouseEventKind::Drag(btn) => (
            match btn {
                MouseButton::Left => 32u32,
                MouseButton::Middle => 33,
                MouseButton::Right => 34,
            },
            false,
        ),
        MouseEventKind::Moved => (35, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        _ => return None,
    };

    let mut button = base_button;
    if mouse.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) {
        button += 4;
    }
    if mouse.modifiers.contains(crossterm::event::KeyModifiers::ALT) {
        button += 8;
    }
    if mouse.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
        button += 16;
    }

    let suffix = if release { 'm' } else { 'M' };
    Some(format!("\x1b[<{};{};{}{}", button, col, row, suffix).into_bytes())
}

/// Whether to hand a mouse event to the embedded tmux client, given the tracking
/// mode that client has actually requested from us. This is the only judgement
/// ttree makes about mouse input in the preview: what the event *means* (focus a
/// pane, start a copy-mode selection, scroll the scrollback) is tmux's call.
/// Without the gate we'd stream every motion/scroll report at clients that never
/// asked for the mouse, and those `\e[<…M` reports leak into the prompt as
/// literal text.
fn should_forward_mouse(kind: &MouseEventKind, mode: vt100::MouseProtocolMode) -> bool {
    use vt100::MouseProtocolMode as M;
    // We never forward bare hover motion. With `mouse on` the embedded client
    // advertises AnyMotion regardless of what the focused app wants, so trusting
    // the mode alone streams hover reports that tmux passes down to an app that
    // never asked, which then leaks them as literal `\e[<35;…M` text into the
    // prompt. Nothing tmux does with the mouse needs buttonless motion.
    if matches!(kind, MouseEventKind::Moved) {
        return false;
    }
    match mode {
        // App wants no mouse input at all.
        M::None => false,
        // Button press/release (and wheel), but never motion.
        M::Press | M::PressRelease => !matches!(kind, MouseEventKind::Drag(_)),
        // Also motion while a button is held (drag), plus buttons and wheel.
        M::ButtonMotion | M::AnyMotion => true,
    }
}

/// Relays OSC 52 clipboard writes out of the embedded tmux client and up to the
/// host terminal. Copying in the preview is tmux's job now, and tmux sets the
/// clipboard by emitting `\e]52;c;<base64>` *at its own client*, which is our
/// PTY rather than the real terminal, so the copy would stop at us unless we
/// pass it on. Sequences straddle 4 KiB read boundaries, hence the running state.
#[derive(Default)]
struct Osc52Relay {
    /// How much of the `\e]52;` introducer we've matched so far.
    prefix: usize,
    /// Payload collected since the introducer, or None when not capturing.
    payload: Option<Vec<u8>>,
    /// We saw an ESC inside the payload: the next byte decides whether it's
    /// an ST terminator (`\e\\`) or just data.
    esc_pending: bool,
}

impl Osc52Relay {
    const INTRO: &'static [u8] = b"\x1b]52;";
    /// Beyond this a "sequence" is junk we mis-latched onto; drop it rather
    /// than buffer the whole session's output.
    const MAX_PAYLOAD: usize = 1 << 20;

    fn feed(&mut self, data: &[u8]) {
        self.scan(data, &mut |payload| Self::emit(payload));
    }

    /// The scanner proper. Split from [`Osc52Relay::feed`] so tests can collect
    /// the sequences instead of writing them at a terminal.
    fn scan(&mut self, data: &[u8], out: &mut impl FnMut(&[u8])) {
        for &b in data {
            match &mut self.payload {
                None => {
                    if b == Self::INTRO[self.prefix] {
                        self.prefix += 1;
                        if self.prefix == Self::INTRO.len() {
                            self.prefix = 0;
                            self.payload = Some(Vec::new());
                        }
                    } else {
                        // Restart the match, allowing for `\e\e]52;`.
                        self.prefix = usize::from(b == Self::INTRO[0]);
                    }
                }
                Some(payload) => {
                    if self.esc_pending {
                        self.esc_pending = false;
                        if b == b'\\' {
                            let done = std::mem::take(payload);
                            self.payload = None;
                            out(&done);
                            continue;
                        }
                        payload.push(0x1b);
                    }
                    match b {
                        0x07 => {
                            let done = std::mem::take(payload);
                            self.payload = None;
                            out(&done);
                        }
                        0x1b => self.esc_pending = true,
                        _ => {
                            payload.push(b);
                            if payload.len() > Self::MAX_PAYLOAD {
                                self.payload = None;
                                self.esc_pending = false;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Written straight to stdout from the PTY reader thread. That races with
    /// ratatui's draws only at write-call boundaries, and one OSC 52 write
    /// neither moves the cursor nor touches SGR state, so a frame can't be
    /// corrupted by landing between two of ratatui's own writes.
    fn emit(payload: &[u8]) {
        use std::io::Write;
        let mut stdout = io::stdout();
        let _ = stdout.write_all(b"\x1b]52;");
        let _ = stdout.write_all(payload);
        let _ = stdout.write_all(b"\x07");
        let _ = stdout.flush();
    }
}

/// The prefix key ttree reserves for itself, taken from the user's own tmux
/// `prefix` option rather than assumed to be C-b.
///
/// This matters most when ttree runs inside ttree: the outer one claims the
/// prefix, so an inner one bound to the same key never sees `prefix d`. Give
/// the two different prefixes and both are reachable. Pressing the prefix twice
/// still forwards one copy down, which is how you reach a tmux, or another
/// ttree, further in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Prefix {
    /// The letter, for matching a key event.
    ch: char,
    /// The control byte a terminal actually sends for it, for forwarding.
    byte: u8,
}

impl Default for Prefix {
    fn default() -> Self {
        // What ttree hardcoded before this was configurable, and tmux's default.
        Prefix { ch: 'b', byte: 0x02 }
    }
}

impl Prefix {
    /// Parse a tmux `prefix` option value such as `C-b` or `C-a`. Anything we
    /// can't map to a control byte (M- bindings, `None`, multi-key prefixes)
    /// falls back to C-b rather than leaving ttree with no prefix at all.
    fn parse(value: Option<String>) -> Self {
        let Some(value) = value else {
            return Prefix::default();
        };
        let Some(rest) = value.trim().strip_prefix("C-") else {
            return Prefix::default();
        };
        let mut chars = rest.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_alphabetic() => {
                Prefix { ch: c.to_ascii_lowercase(), byte: (c.to_ascii_uppercase() as u8) & 0x1f }
            }
            _ => Prefix::default(),
        }
    }

    fn matches(&self, key: &crossterm::event::KeyEvent) -> bool {
        key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&self.ch))
    }
}

fn modifier_code(mods: KeyModifiers) -> u8 {
    let mut m = 1u8;
    if mods.contains(KeyModifiers::SHIFT) {
        m += 1;
    }
    if mods.contains(KeyModifiers::ALT) {
        m += 2;
    }
    if mods.contains(KeyModifiers::CONTROL) {
        m += 4;
    }
    m
}

fn csi_letter(letter: char, m: u8, bytes: &mut Vec<u8>) {
    if m == 1 {
        bytes.extend_from_slice(format!("\x1b[{}", letter).as_bytes());
    } else {
        bytes.extend_from_slice(format!("\x1b[1;{}{}", m, letter).as_bytes());
    }
}

fn csi_tilde(n: u8, m: u8, bytes: &mut Vec<u8>) {
    if m == 1 {
        bytes.extend_from_slice(format!("\x1b[{}~", n).as_bytes());
    } else {
        bytes.extend_from_slice(format!("\x1b[{};{}~", n, m).as_bytes());
    }
}

fn encode_key(key: &crossterm::event::KeyEvent, bytes: &mut Vec<u8>) {
    use crossterm::event::KeyCode;
    let mods = key.modifiers;
    let m = modifier_code(mods);
    let alt = mods.contains(KeyModifiers::ALT);
    let ctrl = mods.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Char(c) => {
            if alt {
                bytes.push(0x1b);
            }
            if ctrl {
                let lc = c.to_ascii_lowercase();
                if lc.is_ascii_lowercase() {
                    bytes.push(lc as u8 - b'a' + 1);
                } else if (b'@'..=b'_').contains(&(c as u8)) {
                    bytes.push(c as u8 - b'@');
                } else if c == ' ' {
                    bytes.push(0);
                } else if c == '?' {
                    bytes.push(127);
                } else if c == '/' {
                    bytes.push(31);
                } else {
                    bytes.extend_from_slice(c.to_string().as_bytes());
                }
            } else {
                bytes.extend_from_slice(c.to_string().as_bytes());
            }
        }
        KeyCode::Enter => {
            if mods.is_empty() {
                bytes.push(b'\r');
            } else {
                // Any modifier + Enter inserts a newline instead of submitting.
                // \n (Ctrl+J, 0x0A) is the one newline byte Claude Code accepts in
                // every terminal and tmux config; the kitty CSI-u form (\x1b[13;2u)
                // is silently dropped by tmux when extended-keys is off, so
                // Shift+Enter would otherwise reach Claude as a plain submit.
                bytes.push(b'\n');
            }
        }
        KeyCode::Esc => {
            if mods.is_empty() {
                bytes.push(27);
            } else if mods == KeyModifiers::ALT {
                bytes.push(0x1b);
                bytes.push(27);
            } else {
                bytes.extend_from_slice(format!("\x1b[27;{}u", m).as_bytes());
            }
        }
        KeyCode::Backspace => {
            if mods.contains(KeyModifiers::SHIFT) {
                bytes.extend_from_slice(format!("\x1b[127;{}u", m).as_bytes());
            } else {
                if alt {
                    bytes.push(0x1b);
                }
                if ctrl {
                    bytes.push(0x08);
                } else {
                    bytes.push(127);
                }
            }
        }
        KeyCode::Tab => {
            if mods.is_empty() {
                bytes.push(b'\t');
            } else if mods == KeyModifiers::SHIFT {
                bytes.extend_from_slice(b"\x1b[Z");
            } else if mods == KeyModifiers::ALT {
                bytes.push(0x1b);
                bytes.push(b'\t');
            } else {
                bytes.extend_from_slice(format!("\x1b[9;{}u", m).as_bytes());
            }
        }
        KeyCode::BackTab => {
            if mods.is_empty() || mods == KeyModifiers::SHIFT {
                bytes.extend_from_slice(b"\x1b[Z");
            } else {
                let mut bm = m;
                if !mods.contains(KeyModifiers::SHIFT) {
                    bm += 1;
                }
                bytes.extend_from_slice(format!("\x1b[9;{}u", bm).as_bytes());
            }
        }
        KeyCode::Up => csi_letter('A', m, bytes),
        KeyCode::Down => csi_letter('B', m, bytes),
        KeyCode::Right => csi_letter('C', m, bytes),
        KeyCode::Left => csi_letter('D', m, bytes),
        KeyCode::Home => csi_letter('H', m, bytes),
        KeyCode::End => csi_letter('F', m, bytes),
        KeyCode::Insert => csi_tilde(2, m, bytes),
        KeyCode::Delete => csi_tilde(3, m, bytes),
        KeyCode::PageUp => csi_tilde(5, m, bytes),
        KeyCode::PageDown => csi_tilde(6, m, bytes),
        KeyCode::F(n) => {
            if m == 1 {
                let s = match n {
                    1 => "\x1bOP",
                    2 => "\x1bOQ",
                    3 => "\x1bOR",
                    4 => "\x1bOS",
                    5 => "\x1b[15~",
                    6 => "\x1b[17~",
                    7 => "\x1b[18~",
                    8 => "\x1b[19~",
                    9 => "\x1b[20~",
                    10 => "\x1b[21~",
                    11 => "\x1b[23~",
                    12 => "\x1b[24~",
                    _ => "",
                };
                bytes.extend_from_slice(s.as_bytes());
            } else {
                let s = match n {
                    1 => format!("\x1b[1;{}P", m),
                    2 => format!("\x1b[1;{}Q", m),
                    3 => format!("\x1b[1;{}R", m),
                    4 => format!("\x1b[1;{}S", m),
                    5 => format!("\x1b[15;{}~", m),
                    6 => format!("\x1b[17;{}~", m),
                    7 => format!("\x1b[18;{}~", m),
                    8 => format!("\x1b[19;{}~", m),
                    9 => format!("\x1b[20;{}~", m),
                    10 => format!("\x1b[21;{}~", m),
                    11 => format!("\x1b[23;{}~", m),
                    12 => format!("\x1b[24;{}~", m),
                    _ => String::new(),
                };
                bytes.extend_from_slice(s.as_bytes());
            }
        }
        _ => {}
    }
}

/// While a session is being previewed we pin its tmux `window-size` to
/// `smallest` so the shared window can't grow past our preview pane and clip the
/// bottom (e.g. an app's input bar). We remember the session's prior setting so
/// we can put it back, and keep it in a static so the panic hook can restore it
/// even if we crash mid-preview. Tuple is (session_id, prior); an empty prior
/// means the option was inherited and should be unset on restore.
static FORCED_WINDOW_SIZE: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

/// Synchronous restore for the panic hook.
fn restore_window_size_blocking(session: &str, prior: &str) {
    let mut cmd = std::process::Command::new("tmux");
    if prior.is_empty() {
        cmd.args(["set-option", "-u", "-t", session, "window-size"]);
    } else {
        cmd.args(["set-option", "-t", session, "window-size", prior]);
    }
    let _ = cmd.output();
}

/// The window and pane a selection points at, for driving the mirror's own
/// current window without touching anyone else's.
fn window_and_pane_of(state: &AppState, id: &str) -> (Option<String>, Option<String>) {
    if let Some(pane) = state.panes.get(id) {
        return (Some(pane.window_id.clone()), Some(id.to_string()));
    }
    if state.windows.contains_key(id) {
        return (Some(id.to_string()), None);
    }
    if let Some(session) = state.sessions.get(id) {
        let window = session
            .windows
            .iter()
            .find(|wid| state.windows.get(*wid).map(|w| w.active).unwrap_or(false))
            .or_else(|| session.windows.first())
            .cloned();
        return (window, None);
    }
    (None, None)
}

/// Point the mirror's own session at a window, and at a pane within it.
///
/// The window is addressed as `<mirror-session>:<window>` so tmux moves *our*
/// session's current window and leaves every other client on the window they
/// were watching. The active pane is a property of the window itself, so that
/// part is still shared, the same as it has always been.
async fn select_in_mirror(mirror: &str, window: Option<&str>, pane: Option<&str>) {
    if let Some(window) = window {
        let _ = tokio::process::Command::new("tmux")
            .args(["select-window", "-t", &format!("{}:{}", mirror, window)])
            .output()
            .await;
    }
    if let Some(pane) = pane {
        let _ =
            tokio::process::Command::new("tmux").args(["select-pane", "-t", pane]).output().await;
    }
}

/// Map a selection id ($session / @window / %pane) to its session id.
fn session_id_of(state: &AppState, id: &str) -> Option<String> {
    if id.starts_with('$') {
        return Some(id.to_string());
    }
    if let Some(w) = state.windows.get(id) {
        return Some(w.session_id.clone());
    }
    if let Some(p) = state.panes.get(id) {
        if let Some(w) = state.windows.get(&p.window_id) {
            return Some(w.session_id.clone());
        }
    }
    None
}

/// Pin a session's window-size to `smallest`, saving the prior session-scoped
/// value (empty = inherited) so it can be restored.
async fn force_window_size_smallest(session: &str) {
    let prior = tokio::process::Command::new("tmux")
        .args(["show-options", "-t", session, "window-size"])
        .output()
        .await
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout).split_whitespace().nth(1).unwrap_or("").to_string()
        })
        .unwrap_or_default();
    if let Ok(mut guard) = FORCED_WINDOW_SIZE.lock() {
        *guard = Some((session.to_string(), prior));
    }
    let _ = tokio::process::Command::new("tmux")
        .args(["set-option", "-t", session, "window-size", "smallest"])
        .output()
        .await;
}

/// Restore (and clear) whatever session we last pinned, if any.
async fn restore_forced_window_size() {
    let taken = FORCED_WINDOW_SIZE.lock().ok().and_then(|mut g| g.take());
    if let Some((session, prior)) = taken {
        if prior.is_empty() {
            let _ = tokio::process::Command::new("tmux")
                .args(["set-option", "-u", "-t", &session, "window-size"])
                .output()
                .await;
        } else {
            let _ = tokio::process::Command::new("tmux")
                .args(["set-option", "-t", &session, "window-size", &prior])
                .output()
                .await;
        }
    }
}

/// Session options ttree pins for as long as it mirrors a session.
///
/// `mouse on` is what makes passthrough mean anything: with the mouse off tmux
/// drops the events, so a drag in the preview selects nothing at all, and since
/// ttree holds the terminal's mouse itself, tmux copy-mode is the only
/// selection the preview can offer.
///
/// `set-clipboard on` is what carries the result back out. tmux only accepts
/// and forwards an application's OSC 52 at `on`; its default `external` drops
/// the sequence, which is exactly where the clipboard dies when a nested ttree
/// relays a copy up through an intermediate server.
const MIRROR_OPTIONS: [(&str, &str); 2] = [("mouse", "on"), ("set-clipboard", "on")];

/// The session whose [`MIRROR_OPTIONS`] we've pinned, and the prior
/// session-scoped value of each (empty = inherited, so restore by unsetting).
/// Kept in a static so the panic hook and the signal handler can put them back
/// even when the normal restore paths never run.
type PinnedOptions = (String, Vec<(String, String)>);
static PINNED_OPTIONS: std::sync::Mutex<Option<PinnedOptions>> = std::sync::Mutex::new(None);

/// Synchronous restore for the panic hook and the signal handler.
fn restore_options_blocking(session: &str, priors: &[(String, String)]) {
    for (name, prior) in priors {
        let mut cmd = std::process::Command::new("tmux");
        if prior.is_empty() {
            cmd.args(["set-option", "-u", "-t", session, name]);
        } else {
            cmd.args(["set-option", "-t", session, name, prior]);
        }
        let _ = cmd.output();
    }
}

/// Pin [`MIRROR_OPTIONS`] on a session, recording each prior session-scoped
/// value (empty = inherited) so it can be restored. Each prior is recorded
/// before that option is changed, so a crash part-way through still leaves the
/// restore paths enough to undo what we actually did.
async fn pin_mirror_options(session: &str) {
    for (name, value) in MIRROR_OPTIONS {
        let prior = tokio::process::Command::new("tmux")
            .args(["show-options", "-t", session, name])
            .output()
            .await
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default();
        if let Ok(mut guard) = PINNED_OPTIONS.lock() {
            let entry = guard.get_or_insert_with(|| (session.to_string(), Vec::new()));
            entry.1.push((name.to_string(), prior));
        }
        let _ = tokio::process::Command::new("tmux")
            .args(["set-option", "-t", session, name, value])
            .output()
            .await;
    }
}

/// Restore (and clear) whatever session's options we last pinned, if any.
async fn restore_pinned_options() {
    let taken = PINNED_OPTIONS.lock().ok().and_then(|mut g| g.take());
    if let Some((session, priors)) = taken {
        for (name, prior) in priors {
            let mut cmd = tokio::process::Command::new("tmux");
            if prior.is_empty() {
                cmd.args(["set-option", "-u", "-t", &session, &name]);
            } else {
                cmd.args(["set-option", "-t", &session, &name, &prior]);
            }
            let _ = cmd.output().await;
        }
    }
}

/// Prefix for the throwaway sessions ttree groups with whatever it mirrors.
/// Also how `sync_state` recognises them, so they never show up in the tree.
const MIRROR_SESSION_PREFIX: &str = "ttree-mirror-";

/// The throwaway session our preview client is attached to, kept in a static so
/// the panic hook and the signal handler can take it down with everything else.
static MIRROR_SESSION: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The session the mirror client actually attaches to: a throwaway session
/// grouped with `target` rather than `target` itself.
///
/// tmux has no per-client current window, it is a property of the session, so
/// attaching straight to the user's session meant every window ttree browsed to
/// dragged their other terminals along with it. A grouped session shares the
/// same windows while keeping its own current window, so we can look around
/// without touching what anyone else is looking at.
///
/// Cleanup is `destroy-unattached on`, but that is set by
/// [`arm_mirror_cleanup`] only once our client is actually on the session:
/// setting it here would have tmux destroy the session immediately, since it is
/// created detached and is therefore already unattached.
async fn create_mirror_session(target: &str) -> Option<String> {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = format!("{}{}-{}", MIRROR_SESSION_PREFIX, std::process::id(), n);

    let output = tokio::process::Command::new("tmux")
        .args(["new-session", "-d", "-t", target, "-s", &name, "-P", "-F", "#{session_id}"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if id.is_empty() {
        return None;
    }
    if let Ok(mut guard) = MIRROR_SESSION.lock() {
        *guard = Some(id.clone());
    }
    Some(id)
}

/// Make the mirror session self-destruct the moment our client leaves it, which
/// covers the cases the explicit teardown cannot: a SIGKILL, or the switch that
/// moves our client to a session grouped with somewhere else.
///
/// Only safe to call once the client is attached. tmux applies
/// `destroy-unattached` immediately, so setting it on the freshly created,
/// still detached session would destroy it before we ever attached, leaving the
/// preview permanently blank.
async fn arm_mirror_cleanup(id: &str) {
    let _ = tokio::process::Command::new("tmux")
        .args(["set-option", "-t", id, "destroy-unattached", "on"])
        .output()
        .await;
}

/// Synchronous teardown for the panic hook and the signal handler.
fn kill_mirror_session_blocking() {
    if let Some(id) = MIRROR_SESSION.lock().ok().and_then(|mut g| g.take()) {
        let _ = std::process::Command::new("tmux").args(["kill-session", "-t", &id]).output();
    }
}

/// Take down the mirror session we last created, if any.
async fn kill_mirror_session() {
    let taken = MIRROR_SESSION.lock().ok().and_then(|mut g| g.take());
    if let Some(id) = taken {
        let _ =
            tokio::process::Command::new("tmux").args(["kill-session", "-t", &id]).output().await;
    }
}

/// The tmux socket ttree was launched against, read from `$TMUX` (the socket
/// path is its first comma-separated field).
///
/// We clear `TMUX` when spawning the embedded client so tmux doesn't refuse to
/// nest, but clearing it also throws away which server we belong to, so on a
/// `tmux -L work` or `tmux -S /path` server every preview quietly attached to
/// the default socket instead: the sidebar listed the right sessions while the
/// preview stayed blank forever. Passing `-S <path>` back puts the client on
/// the server we're actually browsing.
fn tmux_socket() -> Option<&'static str> {
    static TMUX_SOCKET: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    TMUX_SOCKET
        .get_or_init(|| {
            let value = std::env::var("TMUX").ok()?;
            let path = value.split(',').next().unwrap_or_default().trim();
            if path.is_empty() {
                None
            } else {
                Some(path.to_string())
            }
        })
        .as_deref()
}

/// Put every pinned tmux option back, synchronously. Shared by the panic hook
/// and the signal handler, both of which run outside the async restore paths.
fn restore_pins_blocking() {
    kill_mirror_session_blocking();
    if let Some((session, prior)) = FORCED_WINDOW_SIZE.lock().ok().and_then(|mut g| g.take()) {
        restore_window_size_blocking(&session, &prior);
    }
    if let Some((session, priors)) = PINNED_OPTIONS.lock().ok().and_then(|mut g| g.take()) {
        restore_options_blocking(&session, &priors);
    }
}

/// Unpin and leave cleanly when we're killed rather than quit. Closing the
/// terminal window or a plain `kill` would otherwise strand the user's session
/// with `window-size smallest` and the mouse forced on, since the restores at
/// the quit keys and the end of `run_app` never get to run.
fn spawn_signal_restore() {
    use tokio::signal::unix::{signal, SignalKind};
    tokio::spawn(async move {
        let (mut term, mut hup, mut int) = match (
            signal(SignalKind::terminate()),
            signal(SignalKind::hangup()),
            signal(SignalKind::interrupt()),
        ) {
            (Ok(t), Ok(h), Ok(i)) => (t, h, i),
            _ => return,
        };
        let code = tokio::select! {
            _ = term.recv() => 143,
            _ = hup.recv() => 129,
            _ = int.recv() => 130,
        };
        restore_pins_blocking();
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            PopKeyboardEnhancementFlags,
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste
        );
        std::process::exit(code);
    });
}

/// Force a full redraw of our embedded preview client (found by its child pid).
/// tmux's incremental repaint can leave stale cells (old scrollback, a curses
/// dialog, fragments from a size change) in our vt100 mirror; a forced refresh
/// repaints the whole screen and clears them.
async fn refresh_embedded_client(pid: u32) {
    if let Ok(output) = tokio::process::Command::new("tmux")
        .args(["list-clients", "-F", "#{client_pid} #{client_tty}"])
        .output()
        .await
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let pid_s = pid.to_string();
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 && parts[0] == pid_s {
                let _ = tokio::process::Command::new("tmux")
                    .args(["refresh-client", "-t", parts[1]])
                    .output()
                    .await;
                break;
            }
        }
    }
}

async fn run_app(state: &mut AppState) -> Result<()> {
    let last_error: Option<String> = None;
    let mut active_terminal: Option<EmbeddedTerminal> = None;
    let mut pty_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut last_selected_id: Option<String> = None;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    // We never use focus reporting, but something upstream (the terminal itself,
    // mosh, or an outer tmux) may leave mode 1004 enabled. Those `\e[I`/`\e[O`
    // sequences are useless to us and actively harmful: over a laggy link
    // crossterm can split the ESC from the rest, decompose them into bare
    // Esc/`[`/`O` keypresses, and we'd forward that noise into the embedded
    // preview's prompt. Disable focus reporting at the source.
    let _ = execute!(stdout, DisableFocusChange);
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    // Blank the alternate screen before the first draw. ratatui diffs against an
    // assumed-empty buffer and only writes the cells we paint, so any pre-existing
    // output (e.g. the fish/fastfetch login greeting) shows through the cells we
    // leave untouched (the preview panel when no preview is open). Most terminals
    // clear the alt screen on entry, but mosh and GNU screen do not, so do it
    // explicitly here.
    terminal.clear()?;

    let mut last_sync_update = tokio::time::Instant::now();
    let action_attach: Option<String>;

    spawn_signal_restore();

    // If we're inside tmux, we want to start by focusing the active pane.
    // Otherwise, we prefer to keep our previous selection (e.g. after a detach)
    if std::env::var("TMUX").is_ok() {
        state.focus.selected_id = None;
    }
    // Follow the user's tmux prefix instead of assuming C-b.
    let prefix = Prefix::parse(Tmux::show_option_global("prefix").await);

    // Learn which session we're running in, if any, so we never mirror it.
    state.own_session = match std::env::var("TMUX_PANE") {
        Ok(pane) if !pane.is_empty() => Tmux::session_of_pane(&pane).await,
        _ => None,
    };
    let _ = sync_state(state).await;

    let mut last_terminal_size = terminal.size().unwrap_or(ratatui::layout::Size::new(80, 24));
    let mut last_sidebar_cols: u16 = 0;
    let mut dragging_separator = false;
    let mut prefix_pending = false;
    // The session whose options we've currently pinned. None when not pinning.
    let mut pinned_session: Option<String> = None;
    // The throwaway session the mirror client is attached to, and the user
    // session it is grouped with.
    let mut mirror_id: Option<String> = None;
    let mut mirror_target: Option<String> = None;
    // The session whose tmux window-size we've currently pinned to `smallest`
    // (only while actively previewing). None when not pinning.
    let mut forced_session: Option<String> = None;
    // Last time we forced a full repaint of the embedded mirror, to self-heal
    // stale/frozen frames.
    let mut last_mirror_refresh = tokio::time::Instant::now();
    let mut current_switch_task: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        if tokio::time::Instant::now() > last_sync_update + Duration::from_millis(200) {
            let _ = sync_state(state).await;
            last_sync_update = tokio::time::Instant::now();

            // Keep the embedded mirror in sync with the selection and repainting.
            // The one-shot switch on selection-change can be aborted mid-navigation,
            // stranding the client on the wrong session showing a stale frame; an
            // attached client can also stop repainting until something nudges it.
            // So here we (a) follow the client's session into the tree while
            // previewing, (b) re-assert the switch in tree mode if the client
            // drifted off the selected session, and (c) periodically force a full
            // repaint so a stale frame self-heals instead of lingering.
            if let Some(pid) = active_terminal.as_ref().and_then(|t| t.pty_pid) {
                let mut found = false;
                let mut client_session = String::new();
                let mut client_pane = String::new();
                let mut client_tty = String::new();
                if let Ok(output) = tokio::process::Command::new("tmux")
                    .args([
                        "list-clients",
                        "-F",
                        "#{client_pid} #{session_id} #{pane_id} #{client_tty}",
                    ])
                    .output()
                    .await
                {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let pid_s = pid.to_string();
                    for line in stdout.lines() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() == 4 && parts[0] == pid_s {
                            found = true;
                            client_session = parts[1].to_string();
                            client_pane = parts[2].to_string();
                            client_tty = parts[3].to_string();
                            break;
                        }
                    }
                }

                // The client sits in our grouped session, which is deliberately
                // not in the tree, so map it back to the session it groups with
                // before comparing it against anything the user can see. Without
                // this the drift check below would fire every single sync.
                if !client_session.is_empty() && Some(&client_session) == mirror_id.as_ref() {
                    if let Some(target) = &mirror_target {
                        client_session = target.clone();
                    }
                }

                if found && state.focus.panel == Panel::Preview {
                    // Follow whichever session the embedded client is on.
                    let visible = state.get_dynamic_visible_items();
                    let target = if visible.contains(&client_pane) {
                        Some(client_pane.clone())
                    } else {
                        visible.into_iter().find(|id| {
                            if id == &client_session {
                                return true;
                            }
                            if let Some(w) = state.windows.get(id) {
                                return w.session_id == client_session;
                            }
                            if let Some(p) = state.panes.get(id) {
                                return state
                                    .windows
                                    .get(&p.window_id)
                                    .map(|w| w.session_id == client_session)
                                    .unwrap_or(false);
                            }
                            false
                        })
                    };
                    if let Some(id) = target {
                        if state.focus.selected_id.as_deref() != Some(&id) {
                            state.focus.selected_id = Some(id);
                        }
                    }
                } else if found {
                    // Tree mode: if the client drifted off the selected session,
                    // clear the last-selected marker so the switch logic re-runs.
                    let want =
                        state.focus.selected_id.as_deref().and_then(|id| session_id_of(state, id));
                    if let Some(want) = want {
                        if want != client_session {
                            last_selected_id = None;
                        }
                    }
                }

                if found
                    && !client_tty.is_empty()
                    && tokio::time::Instant::now()
                        > last_mirror_refresh + Duration::from_millis(1000)
                {
                    let _ = tokio::process::Command::new("tmux")
                        .args(["refresh-client", "-t", &client_tty])
                        .output()
                        .await;
                    last_mirror_refresh = tokio::time::Instant::now();
                }
            }
        }

        let current_size = terminal.size().unwrap_or(last_terminal_size);

        // Initialize sidebar_cols from default percentage if not yet set
        if state.sidebar_cols == 0 {
            state.sidebar_cols = ((current_size.width as f32 * 0.28) as u16).max(8);
        }

        let sidebar_changed = state.sidebar_cols != last_sidebar_cols;
        if current_size != last_terminal_size || sidebar_changed {
            if let Some(term) = &mut active_terminal {
                let cols = current_size.width.saturating_sub(state.sidebar_cols);
                let rows = current_size.height.saturating_sub(1);
                if cols > 0 && rows > 0 {
                    if let Ok(master) = term.pty_master.lock() {
                        let _ =
                            master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
                    }
                    if let Ok(mut parser) = term.parser.write() {
                        parser.screen_mut().set_size(rows, cols);
                    }
                    let _ = terminal.clear();
                }
            }
            last_terminal_size = current_size;
            last_sidebar_cols = state.sidebar_cols;
        }

        // Never mirror the session ttree is running in. The embedded client
        // would attach to the very window we're drawing, so the preview fills
        // with ttree drawing ttree drawing ttree, and the nested clients fight
        // over the window size.
        state.mirror_suppressed = match (&state.own_session, state.focus.selected_id.as_deref()) {
            (Some(own), Some(id)) => session_id_of(state, id).as_deref() == Some(own.as_str()),
            _ => false,
        };

        if state.focus.selected_id != last_selected_id {
            state.focus.enable_scrolling = true;

            if state.mirror_suppressed {
                if let Some(term) = active_terminal.take() {
                    if let Some(pid) = term.pty_pid {
                        let _ = tokio::process::Command::new("kill")
                            .arg(pid.to_string())
                            .output()
                            .await;
                    }
                }
                if let Some(task) = pty_task.take() {
                    task.abort();
                }
                state.focus.panel = Panel::Tree;
            } else if let Some(target_id) = &state.focus.selected_id {
                if let Some(term) = &mut active_terminal {
                    let fallback_target = target_id.clone();
                    term.target_id = target_id.clone();
                    let want_session = session_id_of(state, target_id);
                    let (window, pane) = window_and_pane_of(state, target_id);

                    if let Some(pid) = term.pty_pid {
                        if let Some(prev) = current_switch_task.take() {
                            prev.abort();
                        }

                        // Staying inside the session we already group with is
                        // just a window change in our own session. Moving to a
                        // different one needs a new grouped session; the old
                        // one destroys itself as soon as our client leaves it.
                        let same_session = want_session.is_some() && want_session == mirror_target;
                        let mirror_for_task = if same_session {
                            mirror_id.clone()
                        } else if let Some(target) = want_session.clone() {
                            let created = create_mirror_session(&target).await;
                            if created.is_some() {
                                mirror_id = created.clone();
                                mirror_target = Some(target);
                            }
                            created
                        } else {
                            None
                        };

                        current_switch_task = Some(tokio::spawn(async move {
                            if let Ok(output) = tokio::process::Command::new("tmux")
                                .args(["list-clients", "-F", "#{client_pid} #{client_tty}"])
                                .output()
                                .await
                            {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let mut client_tty = None;
                                for line in stdout.lines() {
                                    let parts: Vec<&str> = line.split_whitespace().collect();
                                    if parts.len() == 2 && parts[0] == pid.to_string() {
                                        client_tty = Some(parts[1].to_string());
                                        break;
                                    }
                                }

                                if let Some(tty) = client_tty {
                                    match &mirror_for_task {
                                        Some(mirror) => {
                                            let _ = tokio::process::Command::new("tmux")
                                                .args(["switch-client", "-c", &tty, "-t", mirror])
                                                .output()
                                                .await;
                                            select_in_mirror(
                                                mirror,
                                                window.as_deref(),
                                                pane.as_deref(),
                                            )
                                            .await;
                                            arm_mirror_cleanup(mirror).await;
                                        }
                                        // No grouped session (an old tmux, or the
                                        // create failed): fall back to attaching
                                        // straight to the target as ttree used to.
                                        None => {
                                            let _ = tokio::process::Command::new("tmux")
                                                .args([
                                                    "switch-client",
                                                    "-c",
                                                    &tty,
                                                    "-t",
                                                    &fallback_target,
                                                ])
                                                .output()
                                                .await;
                                        }
                                    }
                                    // Force a clean repaint of the new session so
                                    // stale cells from the previous one don't
                                    // bleed into our mirror.
                                    let _ = tokio::process::Command::new("tmux")
                                        .args(["refresh-client", "-t", &tty])
                                        .output()
                                        .await;
                                }
                            }
                        }));
                    }
                } else {
                    let cols = current_size.width.saturating_sub(state.sidebar_cols);
                    let rows = current_size.height.saturating_sub(1);

                    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                    let parser = Arc::new(RwLock::new(vt100::Parser::new(rows, cols, 0)));

                    let pty_system = native_pty_system();
                    let pty_rows = if rows > 0 { rows } else { 24 };
                    let pty_cols = if cols > 0 { cols } else { 80 };

                    if let Ok(pair) = pty_system.openpty(PtySize {
                        rows: pty_rows,
                        cols: pty_cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    }) {
                        let mut cmd = CommandBuilder::new("tmux");
                        // Unset TMUX to prevent nesting issues in the preview,
                        // then name the socket explicitly since unsetting it is
                        // what loses the server (see `tmux_socket`).
                        cmd.env("TMUX", "");
                        cmd.env("TMUX_PANE", "");
                        if let Some(socket) = tmux_socket() {
                            cmd.args(["-S", socket]);
                        }
                        // Attach to a session grouped with the target rather
                        // than to the target itself, so browsing moves our
                        // current window and nobody else's.
                        let want_session = session_id_of(state, target_id);
                        let created = match &want_session {
                            Some(target) => create_mirror_session(target).await,
                            None => None,
                        };
                        if created.is_some() {
                            mirror_id = created.clone();
                            mirror_target = want_session.clone();
                        }
                        let attach_target = created.clone().unwrap_or_else(|| target_id.clone());
                        cmd.args(["attach-session", "-t", &attach_target]);

                        if let Ok(mut child) = pair.slave.spawn_command(cmd) {
                            let pid = child.process_id();
                            drop(pair.slave);

                            let mut reader = pair.master.try_clone_reader().unwrap();
                            let mut writer = pair.master.take_writer().unwrap();

                            active_terminal = Some(EmbeddedTerminal {
                                parser: parser.clone(),
                                pty_writer: tx,
                                target_id: target_id.clone(),
                                pty_pid: pid,
                                pty_master: std::sync::Arc::new(std::sync::Mutex::new(pair.master)),
                            });
                            // Once the client has registered with the server,
                            // force a full repaint so leftover scrollback under
                            // the freshly-attached session doesn't bleed through.
                            let initial =
                                created.clone().map(|m| (m, window_and_pane_of(state, target_id)));
                            if let Some(p) = pid {
                                tokio::spawn(async move {
                                    tokio::time::sleep(Duration::from_millis(200)).await;
                                    // The group's current window is wherever the
                                    // user left it, so put our own session on the
                                    // row that is actually selected.
                                    if let Some((mirror, (window, pane))) = initial {
                                        select_in_mirror(
                                            &mirror,
                                            window.as_deref(),
                                            pane.as_deref(),
                                        )
                                        .await;
                                        arm_mirror_cleanup(&mirror).await;
                                    }
                                    refresh_embedded_client(p).await;
                                });
                            }
                            let mut receiver = rx;
                            let parser_clone = parser.clone();

                            let reader_task = tokio::task::spawn_blocking(move || {
                                let mut buf = [0u8; 4096];
                                let mut clipboard = Osc52Relay::default();
                                loop {
                                    match std::io::Read::read(&mut reader, &mut buf) {
                                        Ok(n) if n > 0 => {
                                            // vt100 drops OSC 52, so scan for it
                                            // before handing the bytes over.
                                            clipboard.feed(&buf[..n]);
                                            if let Ok(mut p) = parser_clone.write() {
                                                p.process(&buf[..n]);
                                            }
                                        }
                                        // EOF: the embedded client exited for good.
                                        Ok(_) => break,
                                        // Transient interruptions must not kill the
                                        // reader: if it dies, the PTY stops being
                                        // drained, tmux suspends the client and the
                                        // mirror freezes. Retry those; only bail on
                                        // a genuine, persistent error.
                                        Err(e)
                                            if matches!(
                                                e.kind(),
                                                std::io::ErrorKind::Interrupted
                                                    | std::io::ErrorKind::WouldBlock
                                            ) =>
                                        {
                                            continue;
                                        }
                                        Err(_) => break,
                                    }
                                }
                            });

                            let writer_task = tokio::task::spawn_blocking(move || {
                                while let Some(data) = receiver.blocking_recv() {
                                    if std::io::Write::write_all(&mut writer, &data).is_err() {
                                        break;
                                    }
                                }
                            });

                            let handle = tokio::task::spawn_blocking(move || {
                                let _ = child.wait();
                                reader_task.abort();
                                writer_task.abort();
                            });
                            pty_task = Some(handle);
                        }
                    }
                }
            }
            last_selected_id = state.focus.selected_id.clone();
        }

        // Clean up pty_task if it finished
        let task_finished = if let Some(task) = &pty_task { task.is_finished() } else { false };
        if task_finished {
            pty_task = None;
            active_terminal = None;
            // Switch back to Tree panel if the terminal exits
            state.focus.panel = Panel::Tree;
        }

        // Pin the previewed session's window-size while actually previewing, so
        // the shared tmux window can't outgrow our pane and clip the input bar.
        // Scoped to the Preview panel only: doing it for the always-on tree
        // mirror would resize co-attached sessions as the user merely browses.
        let desired_forced = if state.focus.panel == Panel::Preview {
            state.focus.selected_id.as_deref().and_then(|id| session_id_of(state, id))
        } else {
            None
        };
        if desired_forced != forced_session {
            if forced_session.is_some() {
                restore_forced_window_size().await;
            }
            if let Some(sess) = &desired_forced {
                force_window_size_smallest(sess).await;
                if let Some(term) = &active_terminal {
                    if let Some(pid) = term.pty_pid {
                        refresh_embedded_client(pid).await;
                    }
                }
            }
            forced_session = desired_forced;
        }

        // Pin the options the mirror needs in whatever session we're showing, and
        // put the user's settings back when we stop. Unlike window-size this
        // isn't scoped to the Preview panel: the preview forwards mouse under
        // tree focus too, and a drag that silently selects nothing is the worse
        // surprise.
        let desired_mirror =
            active_terminal.as_ref().and_then(|t| session_id_of(state, &t.target_id));
        if desired_mirror != pinned_session {
            if pinned_session.is_some() {
                restore_pinned_options().await;
            }
            if let Some(sess) = &desired_mirror {
                pin_mirror_options(sess).await;
            }
            pinned_session = desired_mirror;
        }

        terminal.draw(|f| {
            ui::render(f, state, &active_terminal, &last_error);
        })?;

        if event::poll(Duration::from_millis(50))? {
            let ev = event::read()?;

            // Mouse handling: separator drag takes priority, then preview passthrough
            if let Event::Mouse(mouse) = ev {
                let sep_col = state.sidebar_cols.saturating_sub(1);
                let near_separator = (mouse.column as i32 - sep_col as i32).unsigned_abs() <= 1;

                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) if near_separator => {
                        dragging_separator = true;
                        continue;
                    }
                    MouseEventKind::Drag(MouseButton::Left) if dragging_separator => {
                        let min_cols = 8u16;
                        let max_cols = current_size.width.saturating_sub(20);
                        state.sidebar_cols = (mouse.column + 1).max(min_cols).min(max_cols);
                        state.save_to_disk();
                        continue;
                    }
                    MouseEventKind::Up(MouseButton::Left) if dragging_separator => {
                        dragging_separator = false;
                        continue;
                    }
                    _ => {}
                }

                // Sidebar: click to select, scroll to navigate (works in both Tree and Preview panels)
                if mouse.column < state.sidebar_cols {
                    const TREE_Y_START: u16 = 1; // block title row
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            if mouse.row >= TREE_Y_START
                                && mouse.row < current_size.height.saturating_sub(1)
                            {
                                let relative_row = (mouse.row - TREE_Y_START) as usize;
                                let logical_idx = relative_row + state.focus.scroll_offset;
                                let visible = state.get_dynamic_visible_items();
                                if let Some(id) = visible.get(logical_idx) {
                                    state.focus.selected_id = Some(id.clone());
                                    state.save_to_disk();
                                }
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            state.move_selection_up();
                            state.save_to_disk();
                        }
                        MouseEventKind::ScrollDown => {
                            state.move_selection_down();
                            state.save_to_disk();
                        }
                        _ => {}
                    }
                }

                // Preview area: relay to the embedded tmux client and let tmux
                // decide what the event means. It is a real client, so with
                // `mouse on` it handles pane focus, copy-mode drag-select, wheel
                // scrollback and border-drag resize itself; with mouse off it
                // passes events down to whichever app asked for them.
                if let Some(term) = &active_terminal {
                    if mouse.column >= state.sidebar_cols {
                        let (vt_rows, vt_cols, mouse_mode) = if let Ok(p) = term.parser.read() {
                            let (r, c) = p.screen().size();
                            (r, c, p.screen().mouse_protocol_mode())
                        } else {
                            (0u16, 0u16, vt100::MouseProtocolMode::None)
                        };

                        if vt_rows > 0
                            && vt_cols > 0
                            && should_forward_mouse(&mouse.kind, mouse_mode)
                        {
                            // Clamp into the mirror's grid: our bottom row is
                            // the command bar, which the client has no row for.
                            let mut m = mouse;
                            m.row = m.row.min(vt_rows - 1);
                            m.column = m.column.min(state.sidebar_cols + vt_cols - 1);
                            if let Some(bytes) = encode_mouse(&m, state.sidebar_cols, 0) {
                                let _ = term.pty_writer.send(bytes);
                            }
                        }
                    }
                }

                continue;
            }

            if let Event::Paste(text) = ev {
                // Forward pastes to the embedded preview as one block. Claude Code
                // mishandles real bracketed paste (the \x1b[201~ marker leaks into
                // the prompt and can hang the CLI), so we don't wrap it; instead we
                // translate newlines to \n (Ctrl+J), which Claude inserts as
                // newlines rather than submitting each line as its own prompt.
                if state.focus.panel == Panel::Preview {
                    if let Some(term) = &active_terminal {
                        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
                        let _ = term.pty_writer.send(normalized.into_bytes());
                    }
                } else if let crate::state::InputMode::Renaming { input, .. }
                | crate::state::InputMode::NewSession { input } = &mut state.input_mode
                {
                    // ttree's own single-line fields: keep the text, drop breaks.
                    let cleaned: String =
                        text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                    input.push_str(&cleaned);
                }
                continue;
            }

            if let Event::Key(key) = ev {
                // Rename mode intercepts all input
                if let crate::state::InputMode::Renaming { .. } = &state.input_mode {
                    match key.code {
                        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if let crate::state::InputMode::Renaming { input, .. } =
                                &mut state.input_mode
                            {
                                input.push(c);
                            }
                        }
                        KeyCode::Backspace => {
                            if let crate::state::InputMode::Renaming { input, .. } =
                                &mut state.input_mode
                            {
                                input.pop();
                            }
                        }
                        KeyCode::Enter => {
                            if let crate::state::InputMode::Renaming { session_id, input } =
                                state.input_mode.clone()
                            {
                                let trimmed = input.trim().to_string();
                                if !trimmed.is_empty() {
                                    let _ = tokio::process::Command::new("tmux")
                                        .args(["rename-session", "-t", &session_id, &trimmed])
                                        .output()
                                        .await;
                                }
                            }
                            state.input_mode = crate::state::InputMode::TuiNormal;
                        }
                        KeyCode::Esc => {
                            state.input_mode = crate::state::InputMode::TuiNormal;
                        }
                        _ => {}
                    }
                    continue;
                }

                // New session mode intercepts all input
                if let crate::state::InputMode::NewSession { .. } = &state.input_mode {
                    match key.code {
                        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if let crate::state::InputMode::NewSession { input } =
                                &mut state.input_mode
                            {
                                input.push(c);
                            }
                        }
                        KeyCode::Backspace => {
                            if let crate::state::InputMode::NewSession { input } =
                                &mut state.input_mode
                            {
                                input.pop();
                            }
                        }
                        KeyCode::Enter => {
                            if let crate::state::InputMode::NewSession { input } =
                                state.input_mode.clone()
                            {
                                let trimmed = input.trim().to_string();
                                if !trimmed.is_empty() {
                                    let _ = tokio::process::Command::new("tmux")
                                        .args(["new-session", "-d", "-s", &trimmed])
                                        .output()
                                        .await;
                                }
                            }
                            state.input_mode = crate::state::InputMode::TuiNormal;
                        }
                        KeyCode::Esc => {
                            state.input_mode = crate::state::InputMode::TuiNormal;
                        }
                        _ => {}
                    }
                    continue;
                }

                if key.code == KeyCode::Char('p')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && state.focus.panel == Panel::Tree
                {
                    state.focus.panel = Panel::Preview;
                    continue;
                }

                if state.focus.panel == Panel::Tree {
                    if state.show_help {
                        match key.code {
                            KeyCode::Char('q')
                            | KeyCode::Esc
                            | KeyCode::Enter
                            | KeyCode::Char('?') => {
                                state.show_help = false;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    match key.code {
                        KeyCode::Char('q') => {
                            state.save_to_disk();
                            // This path exits the process outright, so put the
                            // session options back before it does.
                            restore_forced_window_size().await;
                            restore_pinned_options().await;
                            disable_raw_mode()?;
                            execute!(
                                io::stdout(),
                                PopKeyboardEnhancementFlags,
                                LeaveAlternateScreen,
                                DisableMouseCapture,
                                DisableBracketedPaste
                            )?;
                            std::process::exit(0);
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            state.save_to_disk();
                            restore_forced_window_size().await;
                            restore_pinned_options().await;
                            disable_raw_mode()?;
                            execute!(
                                io::stdout(),
                                PopKeyboardEnhancementFlags,
                                LeaveAlternateScreen,
                                DisableMouseCapture,
                                DisableBracketedPaste
                            )?;
                            std::process::exit(0);
                        }
                        KeyCode::Char('?') => {
                            state.show_help = true;
                        }
                        KeyCode::Enter => {
                            state.focus.panel = Panel::Preview;
                        }
                        KeyCode::Char('a') => {
                            if let Some(sel) = &state.focus.selected_id {
                                state.last_target_id = Some(sel.clone());
                                state.save_to_disk();
                                action_attach = Some(sel.clone());
                                break;
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            state.move_selection_up();
                            state.save_to_disk();
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            state.move_selection_down();
                            state.save_to_disk();
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            state.switch_nav_left();
                            state.save_to_disk();
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            state.switch_nav_right();
                            state.save_to_disk();
                        }
                        KeyCode::Char(' ') => {
                            state.toggle_expansion();
                            state.save_to_disk();
                        }
                        KeyCode::Char('n') => {
                            state.input_mode =
                                crate::state::InputMode::NewSession { input: String::new() };
                        }
                        KeyCode::Char('r') => {
                            let session_id = state.focus.selected_id.as_ref().and_then(|sel| {
                                if state.sessions.contains_key(sel) {
                                    Some(sel.clone())
                                } else if let Some(w) = state.windows.get(sel) {
                                    Some(w.session_id.clone())
                                } else if let Some(p) = state.panes.get(sel) {
                                    state.windows.get(&p.window_id).map(|w| w.session_id.clone())
                                } else {
                                    None
                                }
                            });
                            if let Some(sid) = session_id {
                                let current_name = state
                                    .sessions
                                    .get(&sid)
                                    .map(|s| s.name.clone())
                                    .unwrap_or_default();
                                state.input_mode = crate::state::InputMode::Renaming {
                                    session_id: sid,
                                    input: current_name,
                                };
                            }
                        }
                        _ => {}
                    }
                } else if state.focus.panel == Panel::Preview {
                    if prefix_pending {
                        prefix_pending = false;
                        if key.code == KeyCode::Char('d')
                            && !key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            state.focus.panel = Panel::Tree;
                        } else if let Some(term) = &active_terminal {
                            // Forward the swallowed prefix and then this key. Pressing
                            // the prefix twice lands here too, which is what
                            // sends one prefix through to a nested tmux or ttree.
                            let mut bytes = vec![prefix.byte];
                            encode_key(&key, &mut bytes);
                            if !bytes.is_empty() {
                                let _ = term.pty_writer.send(bytes);
                            }
                        }
                    } else if prefix.matches(&key) {
                        prefix_pending = true;
                    } else if let Some(term) = &active_terminal {
                        let mut bytes = Vec::new();
                        encode_key(&key, &mut bytes);
                        if !bytes.is_empty() {
                            let _ = term.pty_writer.send(bytes);
                        }
                    }
                }
            }
        }
    }

    // Put back the tmux options we pinned for previewing.
    restore_forced_window_size().await;
    restore_pinned_options().await;
    kill_mirror_session().await;

    if let Some(term) = active_terminal.take() {
        if let Some(pid) = term.pty_pid {
            let _ = tokio::process::Command::new("kill").arg(pid.to_string()).output().await;
        }
    }

    if let Some(task) = pty_task.take() {
        task.abort();
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        PopKeyboardEnhancementFlags,
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    if let Some(target) = action_attach {
        let mut cmd = tokio::process::Command::new("tmux");
        if std::env::var("TMUX").is_ok() {
            cmd.args(["switch-client", "-t", &target]);
        } else {
            cmd.env_remove("TMUX").env_remove("TMUX_PANE");
            if let Some(socket) = tmux_socket() {
                cmd.args(["-S", socket]);
            }
            cmd.args(["attach-session", "-t", &target]);
        }
        let mut child = cmd.spawn()?;
        let _ = child.wait().await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, MouseEvent};

    fn mouse(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
        MouseEvent { kind, column, row, modifiers }
    }

    fn encoded(kind: MouseEventKind, column: u16, row: u16, x_offset: u16) -> String {
        let ev = mouse(kind, column, row, KeyModifiers::NONE);
        String::from_utf8(encode_mouse(&ev, x_offset, 0).expect("encodable")).unwrap()
    }

    #[test]
    fn mouse_encodes_sgr_with_one_based_coordinates() {
        // Column 0 of the preview is column 1 to the embedded client.
        assert_eq!(encoded(MouseEventKind::Down(MouseButton::Left), 14, 0, 14), "\x1b[<0;1;1M");
        assert_eq!(encoded(MouseEventKind::Down(MouseButton::Right), 20, 4, 14), "\x1b[<2;7;5M");
    }

    #[test]
    fn mouse_release_uses_lowercase_terminator() {
        assert_eq!(encoded(MouseEventKind::Up(MouseButton::Left), 15, 2, 14), "\x1b[<0;2;3m");
    }

    #[test]
    fn mouse_drag_and_wheel_use_their_own_button_codes() {
        assert_eq!(encoded(MouseEventKind::Drag(MouseButton::Left), 15, 0, 14), "\x1b[<32;2;1M");
        assert_eq!(encoded(MouseEventKind::ScrollUp, 15, 0, 14), "\x1b[<64;2;1M");
        assert_eq!(encoded(MouseEventKind::ScrollDown, 15, 0, 14), "\x1b[<65;2;1M");
    }

    #[test]
    fn mouse_modifiers_add_their_bits() {
        let shift_alt_ctrl = KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL;
        let ev = mouse(MouseEventKind::Down(MouseButton::Left), 14, 0, shift_alt_ctrl);
        let out = String::from_utf8(encode_mouse(&ev, 14, 0).unwrap()).unwrap();
        assert_eq!(out, "\x1b[<28;1;1M"); // 0 + 4 + 8 + 16
    }

    #[test]
    fn forwarding_follows_the_mode_the_client_asked_for() {
        use vt100::MouseProtocolMode as M;
        let down = MouseEventKind::Down(MouseButton::Left);
        let drag = MouseEventKind::Drag(MouseButton::Left);

        // Asked for nothing: send nothing.
        assert!(!should_forward_mouse(&down, M::None));
        assert!(!should_forward_mouse(&MouseEventKind::ScrollUp, M::None));

        // Buttons and wheel, but not drag.
        assert!(should_forward_mouse(&down, M::Press));
        assert!(should_forward_mouse(&MouseEventKind::ScrollUp, M::PressRelease));
        assert!(!should_forward_mouse(&drag, M::Press));
        assert!(!should_forward_mouse(&drag, M::PressRelease));

        // Drag once motion is wanted.
        assert!(should_forward_mouse(&drag, M::ButtonMotion));
        assert!(should_forward_mouse(&drag, M::AnyMotion));
    }

    #[test]
    fn bare_hover_is_never_forwarded() {
        // Even under AnyMotion: tmux advertises it whenever `mouse on`,
        // regardless of what the app in the pane actually wants.
        use vt100::MouseProtocolMode as M;
        for mode in [M::None, M::Press, M::PressRelease, M::ButtonMotion, M::AnyMotion] {
            assert!(!should_forward_mouse(&MouseEventKind::Moved, mode));
        }
    }

    fn relayed(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut relay = Osc52Relay::default();
        let mut out = Vec::new();
        for chunk in chunks {
            relay.scan(chunk, &mut |payload| out.push(payload.to_vec()));
        }
        out
    }

    #[test]
    fn osc52_relays_a_bel_terminated_sequence() {
        assert_eq!(relayed(&[b"junk\x1b]52;c;aGk=\x07more"]), vec![b"c;aGk=".to_vec()]);
    }

    #[test]
    fn osc52_relays_an_st_terminated_sequence() {
        assert_eq!(relayed(&[b"\x1b]52;c;YWJj\x1b\\rest"]), vec![b"c;YWJj".to_vec()]);
    }

    #[test]
    fn osc52_survives_every_split_point() {
        // The PTY hands us 4 KiB at a time, so a sequence can be cut anywhere.
        let data = b"pre\x1b]52;c;YWJj\x1b\\post";
        for split in 1..data.len() {
            assert_eq!(
                relayed(&[&data[..split], &data[split..]]),
                vec![b"c;YWJj".to_vec()],
                "split at {}",
                split
            );
        }
    }

    #[test]
    fn osc52_ignores_other_sequences() {
        assert!(relayed(&[b"\x1b]0;title\x07\x1b]52\x07\x1b[<0;5;5M"]).is_empty());
    }

    #[test]
    fn osc52_restarts_the_introducer_on_a_repeated_escape() {
        assert_eq!(relayed(&[b"\x1b\x1b]52;c;YQ==\x07"]), vec![b"c;YQ==".to_vec()]);
    }

    #[test]
    fn osc52_keeps_an_escape_that_is_not_a_terminator() {
        assert_eq!(relayed(&[b"\x1b]52;c;a\x1bb\x07"]), vec![b"c;a\x1bb".to_vec()]);
    }

    #[test]
    fn osc52_drops_an_oversized_payload_and_recovers() {
        let mut relay = Osc52Relay::default();
        let mut out: Vec<Vec<u8>> = Vec::new();
        relay.scan(b"\x1b]52;", &mut |p| out.push(p.to_vec()));
        relay.scan(&vec![b'A'; Osc52Relay::MAX_PAYLOAD + 1], &mut |p| out.push(p.to_vec()));
        assert!(out.is_empty(), "runaway payload should be dropped");
        relay.scan(b"\x1b]52;c;YQ==\x07", &mut |p| out.push(p.to_vec()));
        assert_eq!(out, vec![b"c;YQ==".to_vec()]);
    }

    fn key_bytes(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
        let mut bytes = Vec::new();
        encode_key(&KeyEvent::new(code, modifiers), &mut bytes);
        bytes
    }

    #[test]
    fn keys_encode_plain_and_modified_forms() {
        assert_eq!(key_bytes(KeyCode::Char('a'), KeyModifiers::NONE), b"a");
        assert_eq!(key_bytes(KeyCode::Char('c'), KeyModifiers::CONTROL), vec![0x03]);
        assert_eq!(key_bytes(KeyCode::Enter, KeyModifiers::NONE), b"\r");
        assert_eq!(key_bytes(KeyCode::Tab, KeyModifiers::SHIFT), b"\x1b[Z");
        // Arrows go plain unmodified, and CSI 1;<mod> when modified.
        assert_eq!(key_bytes(KeyCode::Up, KeyModifiers::NONE), b"\x1b[A");
        assert_eq!(key_bytes(KeyCode::Up, KeyModifiers::CONTROL), b"\x1b[1;5A");
    }
}

#[cfg(test)]
mod prefix_tests {
    use super::*;
    use crossterm::event::KeyEvent;

    #[test]
    fn prefix_follows_the_tmux_option() {
        assert_eq!(Prefix::parse(Some("C-a".into())), Prefix { ch: 'a', byte: 0x01 });
        assert_eq!(Prefix::parse(Some("C-b".into())), Prefix { ch: 'b', byte: 0x02 });
        assert_eq!(Prefix::parse(Some(" C-z \n".into())), Prefix { ch: 'z', byte: 0x1a });
    }

    #[test]
    fn unparseable_prefixes_fall_back_to_c_b() {
        // Better a working default than a ttree with no prefix at all.
        for value in [None, Some("None".into()), Some("M-a".into()), Some("C-Space".into())] {
            assert_eq!(Prefix::parse(value), Prefix::default());
        }
    }

    #[test]
    fn prefix_matches_only_with_control() {
        let p = Prefix::parse(Some("C-a".into()));
        assert!(p.matches(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)));
        assert!(!p.matches(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)));
        assert!(!p.matches(&KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)));
    }
}
