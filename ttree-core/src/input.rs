//! The terminal input-encoding layer: SGR mouse encoding, the forwarding
//! gate, the OSC 52 clipboard scanner, key encoding, and the tmux-prefix
//! matcher. Moved out of `ttree`'s `main.rs` so `ttree-tile` can reuse it -
//! see `docs/superpowers/specs/2026-09-12-i3-tiling-design.md`.

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use std::io;

pub fn encode_mouse(
    mouse: &crossterm::event::MouseEvent,
    x_offset: u16,
    y_offset: u16,
) -> Option<Vec<u8>> {
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
pub fn should_forward_mouse(kind: &MouseEventKind, mode: vt100::MouseProtocolMode) -> bool {
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
pub struct Osc52Relay {
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

    pub fn feed(&mut self, data: &[u8]) {
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
pub struct Prefix {
    /// The letter, for matching a key event.
    pub ch: char,
    /// The control byte a terminal actually sends for it, for forwarding.
    pub byte: u8,
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
    pub fn parse(value: Option<String>) -> Self {
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

    pub fn matches(&self, key: &crossterm::event::KeyEvent) -> bool {
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

pub fn encode_key(key: &crossterm::event::KeyEvent, bytes: &mut Vec<u8>) {
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
