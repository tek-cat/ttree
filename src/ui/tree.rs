use crate::state::AppState;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use std::collections::HashSet;

pub struct TreeWidget<'a> {
    state: &'a AppState,
}

impl<'a> TreeWidget<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }
}

fn sel_style(is_selected: bool, selection_bg: Color) -> Style {
    let bg = if is_selected { selection_bg } else { Color::Reset };
    let mut s = Style::default().bg(bg);
    if is_selected {
        s = s.add_modifier(Modifier::BOLD);
    }
    s
}

impl<'a> Widget for TreeWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;
        let mut line_idx: usize = 0;
        let scroll = self.state.focus.scroll_offset;
        let icon_color = self.state.theme.icon;
        let active_fg = self.state.theme.active;
        let selection_bg = self.state.theme.selection_bg;

        // Ask the model which rows survive the filter instead of re-running the
        // match here: navigation, scrolling and drawing all index the same list,
        // and a second opinion about it would put the selection off screen.
        let visible: Option<HashSet<String>> = self
            .state
            .filter_active()
            .then(|| self.state.get_dynamic_visible_items().into_iter().collect());
        let keep = |id: &str| visible.as_ref().is_none_or(|v| v.contains(id));

        // A filter matching nothing empties the sidebar, which reads as a crash
        // rather than as a search miss unless we say so.
        if visible.as_ref().is_some_and(|v| v.is_empty()) {
            let note = Line::from(Span::styled(
                " no matches",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            ));
            buf.set_line(area.x, area.y, &note, area.width);
            return;
        }

        for session in self.state.sessions.values() {
            if y >= area.bottom() {
                break;
            }

            let num_windows = session.windows.len();
            let total_panes: usize = session
                .windows
                .iter()
                .filter_map(|wid| self.state.windows.get(wid))
                .map(|w| w.panes.len())
                .sum();

            if num_windows <= 1 && total_panes <= 1 {
                // ── Case A: single leaf row ──────────────────────────────────
                let leaf_id = session
                    .windows
                    .first()
                    .and_then(|wid| self.state.windows.get(wid))
                    .and_then(|w| w.panes.first())
                    .cloned()
                    .unwrap_or(session.id.clone());

                let pane = session
                    .windows
                    .first()
                    .and_then(|wid| self.state.windows.get(wid))
                    .and_then(|w| w.panes.first())
                    .and_then(|pid| self.state.panes.get(pid));

                if !keep(&leaf_id) {
                    continue;
                }

                let label = session.name.clone();
                let is_active = pane.map(|p| p.active).unwrap_or(session.attached);
                let is_sel = Some(&leaf_id) == self.state.focus.selected_id.as_ref();

                if line_idx >= scroll {
                    let style = sel_style(is_sel, selection_bg);
                    let fg = if is_active { active_fg } else { Color::Reset };
                    let line = Line::from(vec![
                        Span::styled("► ", style.fg(icon_color)),
                        Span::styled(label, style.fg(fg)),
                    ]);
                    buf.set_line(area.x, y, &line, area.width);
                    y += 1;
                }
                line_idx += 1;
            } else if num_windows == 1 {
                // ── Case B: session header + panes directly ──────────────────
                // Dropping the header drops its panes with it: the model keeps
                // the ancestors of every match, so a hidden header has no
                // matching panes underneath.
                if !keep(&session.id) {
                    continue;
                }

                let is_sel = Some(&session.id) == self.state.focus.selected_id.as_ref();

                if line_idx >= scroll {
                    let style = sel_style(is_sel, selection_bg);
                    let icon = if session.expanded { "▼" } else { "▶" };
                    let attached = if session.attached { " (attached)" } else { "" };
                    let line = Line::from(vec![
                        Span::styled(format!("{} ", icon), style.fg(icon_color)),
                        Span::styled(session.name.clone(), style),
                        Span::styled(
                            format!(" ({}{})", total_panes, attached),
                            style.fg(Color::Gray),
                        ),
                    ]);
                    buf.set_line(area.x, y, &line, area.width);
                    y += 1;
                }
                line_idx += 1;

                if session.expanded {
                    if let Some(window) =
                        session.windows.first().and_then(|wid| self.state.windows.get(wid))
                    {
                        // With a filter on, the last row drawn is not
                        // necessarily the last pane, so the corner connector has
                        // to track the surviving rows.
                        let last_kept = window.panes.iter().rposition(|pid| keep(pid));
                        for (idx, pid) in window.panes.iter().enumerate() {
                            if y >= area.bottom() {
                                break;
                            }
                            if !keep(pid) {
                                continue;
                            }
                            if let Some(pane) = self.state.panes.get(pid) {
                                if line_idx >= scroll {
                                    let is_psel =
                                        Some(pid) == self.state.focus.selected_id.as_ref();
                                    let style = sel_style(is_psel, selection_bg);
                                    let fg = if pane.active { active_fg } else { Color::Reset };
                                    let connector = if Some(idx) == last_kept {
                                        "  └─ "
                                    } else {
                                        "  ├─ "
                                    };
                                    let line = Line::from(vec![
                                        Span::styled(connector, style),
                                        Span::styled(pane.display_name().to_string(), style.fg(fg)),
                                    ]);
                                    buf.set_line(area.x, y, &line, area.width);
                                    y += 1;
                                }
                                line_idx += 1;
                            }
                        }
                    }
                }
            } else {
                // ── Case C: full hierarchy ───────────────────────────────────
                if !keep(&session.id) {
                    continue;
                }

                let is_sel = Some(&session.id) == self.state.focus.selected_id.as_ref();

                if line_idx >= scroll {
                    let style = sel_style(is_sel, selection_bg);
                    let icon = if session.expanded { "▼" } else { "▶" };
                    let attached = if session.attached { " (attached)" } else { "" };
                    let line = Line::from(vec![
                        Span::styled(format!("{} ", icon), style.fg(icon_color)),
                        Span::styled(session.name.clone(), style),
                        Span::styled(
                            format!(" ({}{})", num_windows, attached),
                            style.fg(Color::Gray),
                        ),
                    ]);
                    buf.set_line(area.x, y, &line, area.width);
                    y += 1;
                }
                line_idx += 1;

                if session.expanded {
                    for wid in &session.windows {
                        if y >= area.bottom() {
                            break;
                        }
                        if let Some(window) = self.state.windows.get(wid) {
                            let num_panes = window.panes.len();

                            if num_panes <= 1 {
                                // Window is a leaf — show pane name
                                let leaf_id = window.panes.first().cloned().unwrap_or(wid.clone());
                                if !keep(&leaf_id) {
                                    continue;
                                }
                                let is_wsel =
                                    Some(&leaf_id) == self.state.focus.selected_id.as_ref();
                                if line_idx >= scroll {
                                    let style = sel_style(is_wsel, selection_bg);
                                    let (label, is_active) = window
                                        .panes
                                        .first()
                                        .and_then(|pid| self.state.panes.get(pid))
                                        .map(|p| (p.display_name().to_string(), p.active))
                                        .unwrap_or_else(|| (window.name.clone(), window.active));
                                    let fg = if is_active { active_fg } else { Color::Reset };
                                    let line = Line::from(vec![
                                        Span::styled("  ► ", style.fg(icon_color)),
                                        Span::styled(label, style.fg(fg)),
                                    ]);
                                    buf.set_line(area.x, y, &line, area.width);
                                    y += 1;
                                }
                                line_idx += 1;
                            } else {
                                // Window is a header (expandable)
                                if !keep(wid) {
                                    continue;
                                }
                                let is_wsel = Some(wid) == self.state.focus.selected_id.as_ref();
                                if line_idx >= scroll {
                                    let style = sel_style(is_wsel, selection_bg);
                                    let fg = if window.active { active_fg } else { Color::Reset };
                                    let wicon = if window.expanded { "▼" } else { "▶" };
                                    let line = Line::from(vec![
                                        Span::styled(format!("  {} ", wicon), style.fg(icon_color)),
                                        Span::styled(window.name.clone(), style.fg(fg)),
                                    ]);
                                    buf.set_line(area.x, y, &line, area.width);
                                    y += 1;
                                }
                                line_idx += 1;

                                if window.expanded {
                                    let last_kept = window.panes.iter().rposition(|pid| keep(pid));
                                    for (idx, pid) in window.panes.iter().enumerate() {
                                        if y >= area.bottom() {
                                            break;
                                        }
                                        if !keep(pid) {
                                            continue;
                                        }
                                        if let Some(pane) = self.state.panes.get(pid) {
                                            if line_idx >= scroll {
                                                let is_psel = Some(pid)
                                                    == self.state.focus.selected_id.as_ref();
                                                let style = sel_style(is_psel, selection_bg);
                                                let fg = if pane.active {
                                                    active_fg
                                                } else {
                                                    Color::Reset
                                                };
                                                let connector = if Some(idx) == last_kept {
                                                    "    └─ "
                                                } else {
                                                    "    ├─ "
                                                };
                                                let line = Line::from(vec![
                                                    Span::styled(connector, style),
                                                    Span::styled(
                                                        pane.display_name().to_string(),
                                                        style.fg(fg),
                                                    ),
                                                ]);
                                                buf.set_line(area.x, y, &line, area.width);
                                                y += 1;
                                            }
                                            line_idx += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
