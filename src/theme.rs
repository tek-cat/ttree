//! Adopts the user's tmux status-bar colors so ttree's chrome matches their
//! terminal at startup. Values are read once from the live tmux server (see
//! [`Theme::from_tmux`]); every field falls back to ttree's historical default
//! when the option is unset or unparseable, so an unthemed tmux looks unchanged.

use crate::tmux_client::Tmux;
use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct Theme {
    /// Command-bar background (tmux `status-style` bg).
    pub bar_bg: Color,
    /// Command-bar foreground (tmux `status-style` fg).
    pub bar_fg: Color,
    /// Mode-label pill background (tmux `window-status-current-style` bg).
    pub pill_bg: Color,
    /// Mode-label pill foreground (tmux `window-status-current-style` fg).
    pub pill_fg: Color,
    /// Tree expand/collapse icons (tmux `status-left` fg).
    pub icon: Color,
    /// Active session/window/pane (tmux `status-right` fg).
    pub active: Color,
    /// Selected tree row background. Deliberately a dimmed neutral rather than
    /// the loud current-window color, so accent text stays readable.
    pub selection_bg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        // These mirror the colors ttree hard-coded before theme support, so a
        // tmux with no custom status styling renders exactly as it used to.
        Self {
            bar_bg: Color::Green,
            bar_fg: Color::Black,
            pill_bg: Color::Green,
            pill_fg: Color::Black,
            icon: Color::Yellow,
            active: Color::Cyan,
            selection_bg: Color::DarkGray,
        }
    }
}

impl Theme {
    /// Build a theme from the running tmux server's global options. Never fails:
    /// missing or unparseable options leave the corresponding default in place.
    pub async fn from_tmux() -> Self {
        let mut t = Theme::default();

        if let Some(s) = Tmux::show_option_global("status-style").await {
            let (fg, bg) = parse_style(&s);
            if let Some(fg) = fg {
                t.bar_fg = fg;
            }
            if let Some(bg) = bg {
                t.bar_bg = bg;
            }
        }

        if let Some(s) = Tmux::show_option_global("window-status-current-style").await {
            let (fg, bg) = parse_style(&s);
            if let Some(fg) = fg {
                t.pill_fg = fg;
            }
            if let Some(bg) = bg {
                t.pill_bg = bg;
            }
        }

        if let Some(s) = Tmux::show_option_global("status-left").await {
            if let Some(c) = first_fg_in_format(&s) {
                t.icon = c;
            }
        }

        if let Some(s) = Tmux::show_option_global("status-right").await {
            if let Some(c) = first_fg_in_format(&s) {
                t.active = c;
            }
        }

        t
    }
}

/// Split a tmux style value like `bg=#191724,fg=#e0def4,bold` into `(fg, bg)`.
/// Unknown tokens (attributes such as `bold`) are ignored. A field is `None`
/// when absent or unparseable, leaving the caller's default untouched.
fn parse_style(s: &str) -> (Option<Color>, Option<Color>) {
    let mut fg = None;
    let mut bg = None;
    for tok in s.split(',') {
        let tok = tok.trim();
        if let Some(v) = tok.strip_prefix("fg=") {
            fg = parse_color(v);
        } else if let Some(v) = tok.strip_prefix("bg=") {
            bg = parse_color(v);
        }
    }
    (fg, bg)
}

/// Pull the first foreground color out of a tmux format string such as
/// `#[fg=#31748f,bold] #S `. Scans each `#[...]` block until one yields an `fg=`.
fn first_fg_in_format(s: &str) -> Option<Color> {
    let mut rest = s;
    while let Some(open) = rest.find("#[") {
        let after = &rest[open + 2..];
        let close = after.find(']')?;
        if let Some(c) = parse_style(&after[..close]).0 {
            return Some(c);
        }
        rest = &after[close + 1..];
    }
    None
}

/// Parse a single tmux color token into a ratatui [`Color`]. Handles hex
/// (`#rrggbb`), indexed (`colour123`/`color123`/bare `123`), the standard named
/// colors and their `bright*` variants, and `default` (terminal default).
fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s == "default" {
        return Some(Color::Reset);
    }
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }
    if let Some(n) = s.strip_prefix("colour").or_else(|| s.strip_prefix("color")) {
        return n.parse::<u8>().ok().map(Color::Indexed);
    }
    if let Ok(n) = s.parse::<u8>() {
        return Some(Color::Indexed(n));
    }
    Some(match s {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::Gray,
        "brightblack" => Color::DarkGray,
        "brightred" => Color::LightRed,
        "brightgreen" => Color::LightGreen,
        "brightyellow" => Color::LightYellow,
        "brightblue" => Color::LightBlue,
        "brightmagenta" => Color::LightMagenta,
        "brightcyan" => Color::LightCyan,
        "brightwhite" => Color::White,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex() {
        assert_eq!(parse_color("#191724"), Some(Color::Rgb(0x19, 0x17, 0x24)));
    }

    #[test]
    fn parses_indexed_forms() {
        assert_eq!(parse_color("colour234"), Some(Color::Indexed(234)));
        assert_eq!(parse_color("color8"), Some(Color::Indexed(8)));
        assert_eq!(parse_color("7"), Some(Color::Indexed(7)));
    }

    #[test]
    fn parses_named_and_default() {
        assert_eq!(parse_color("cyan"), Some(Color::Cyan));
        assert_eq!(parse_color("brightblack"), Some(Color::DarkGray));
        assert_eq!(parse_color("default"), Some(Color::Reset));
        assert_eq!(parse_color("bogus"), None);
    }

    #[test]
    fn parses_style_value() {
        let (fg, bg) = parse_style("bg=#191724,fg=#e0def4");
        assert_eq!(bg, Some(Color::Rgb(0x19, 0x17, 0x24)));
        assert_eq!(fg, Some(Color::Rgb(0xe0, 0xde, 0xf4)));
    }

    #[test]
    fn ignores_attributes_in_style() {
        let (fg, bg) = parse_style("fg=#31748f,bold");
        assert_eq!(fg, Some(Color::Rgb(0x31, 0x74, 0x8f)));
        assert_eq!(bg, None);
    }

    #[test]
    fn extracts_fg_from_format() {
        assert_eq!(
            first_fg_in_format("#[fg=#31748f,bold] #S "),
            Some(Color::Rgb(0x31, 0x74, 0x8f))
        );
        // Skips a leading block with no fg, finds the next.
        assert_eq!(first_fg_in_format("#[bold]#[fg=cyan]x"), Some(Color::Cyan));
        assert_eq!(first_fg_in_format("plain text"), None);
    }
}
