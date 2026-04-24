use crate::state::{AppState, Pane};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

pub struct TreeWidget<'a> {
    state: &'a AppState,
}

impl<'a> TreeWidget<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }
}

fn pane_name(pane: &Pane) -> &str {
    if !pane.current_command.is_empty() {
        &pane.current_command
    } else {
        &pane.title
    }
}

fn sel_style(is_selected: bool) -> Style {
    let bg = if is_selected { Color::DarkGray } else { Color::Reset };
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

        for session in self.state.sessions.values() {
            if y >= area.bottom() {
                break;
            }

            let num_windows = session.windows.len();
            let total_panes: usize = session.windows.iter()
                .filter_map(|wid| self.state.windows.get(wid))
                .map(|w| w.panes.len())
                .sum();

            if num_windows <= 1 && total_panes <= 1 {
                // ── Case A: single leaf row ──────────────────────────────────
                let leaf_id = session.windows.first()
                    .and_then(|wid| self.state.windows.get(wid))
                    .and_then(|w| w.panes.first())
                    .cloned()
                    .unwrap_or(session.id.clone());

                let pane = session.windows.first()
                    .and_then(|wid| self.state.windows.get(wid))
                    .and_then(|w| w.panes.first())
                    .and_then(|pid| self.state.panes.get(pid));

                let label = session.name.clone();
                let is_active = pane.map(|p| p.active).unwrap_or(session.attached);
                let is_sel = Some(&leaf_id) == self.state.focus.selected_id.as_ref();

                if line_idx >= scroll {
                    let style = sel_style(is_sel);
                    let fg = if is_active { Color::Cyan } else { Color::Reset };
                    let line = Line::from(vec![
                        Span::styled("► ", style.fg(Color::Yellow)),
                        Span::styled(label, style.fg(fg)),
                    ]);
                    buf.set_line(area.x, y, &line, area.width);
                    y += 1;
                }
                line_idx += 1;

            } else if num_windows == 1 {
                // ── Case B: session header + panes directly ──────────────────
                let is_sel = Some(&session.id) == self.state.focus.selected_id.as_ref();

                if line_idx >= scroll {
                    let style = sel_style(is_sel);
                    let icon = if session.expanded { "▼" } else { "▶" };
                    let attached = if session.attached { " (attached)" } else { "" };
                    let line = Line::from(vec![
                        Span::styled(format!("{} ", icon), style.fg(Color::Yellow)),
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
                    if let Some(window) = session.windows.first()
                        .and_then(|wid| self.state.windows.get(wid))
                    {
                        let num_panes = window.panes.len();
                        for (idx, pid) in window.panes.iter().enumerate() {
                            if y >= area.bottom() {
                                break;
                            }
                            if let Some(pane) = self.state.panes.get(pid) {
                                if line_idx >= scroll {
                                    let is_psel = Some(pid) == self.state.focus.selected_id.as_ref();
                                    let style = sel_style(is_psel);
                                    let fg = if pane.active { Color::Cyan } else { Color::Reset };
                                    let connector = if idx == num_panes - 1 { "  └─ " } else { "  ├─ " };
                                    let line = Line::from(vec![
                                        Span::styled(connector, style),
                                        Span::styled(pane_name(pane).to_string(), style.fg(fg)),
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
                let is_sel = Some(&session.id) == self.state.focus.selected_id.as_ref();

                if line_idx >= scroll {
                    let style = sel_style(is_sel);
                    let icon = if session.expanded { "▼" } else { "▶" };
                    let attached = if session.attached { " (attached)" } else { "" };
                    let line = Line::from(vec![
                        Span::styled(format!("{} ", icon), style.fg(Color::Yellow)),
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
                                let is_wsel = Some(&leaf_id) == self.state.focus.selected_id.as_ref();
                                if line_idx >= scroll {
                                    let style = sel_style(is_wsel);
                                    let (label, is_active) = window.panes.first()
                                        .and_then(|pid| self.state.panes.get(pid))
                                        .map(|p| (pane_name(p).to_string(), p.active))
                                        .unwrap_or_else(|| (window.name.clone(), window.active));
                                    let fg = if is_active { Color::Cyan } else { Color::Reset };
                                    let line = Line::from(vec![
                                        Span::styled("  ► ", style.fg(Color::Yellow)),
                                        Span::styled(label, style.fg(fg)),
                                    ]);
                                    buf.set_line(area.x, y, &line, area.width);
                                    y += 1;
                                }
                                line_idx += 1;
                            } else {
                                // Window is a header (expandable)
                                let is_wsel = Some(wid) == self.state.focus.selected_id.as_ref();
                                if line_idx >= scroll {
                                    let style = sel_style(is_wsel);
                                    let fg = if window.active { Color::Cyan } else { Color::Reset };
                                    let wicon = if window.expanded { "▼" } else { "▶" };
                                    let line = Line::from(vec![
                                        Span::styled(format!("  {} ", wicon), style.fg(Color::Yellow)),
                                        Span::styled(window.name.clone(), style.fg(fg)),
                                    ]);
                                    buf.set_line(area.x, y, &line, area.width);
                                    y += 1;
                                }
                                line_idx += 1;

                                if window.expanded {
                                    for (idx, pid) in window.panes.iter().enumerate() {
                                        if y >= area.bottom() {
                                            break;
                                        }
                                        if let Some(pane) = self.state.panes.get(pid) {
                                            if line_idx >= scroll {
                                                let is_psel = Some(pid) == self.state.focus.selected_id.as_ref();
                                                let style = sel_style(is_psel);
                                                let fg = if pane.active { Color::Cyan } else { Color::Reset };
                                                let connector = if idx == num_panes - 1 { "    └─ " } else { "    ├─ " };
                                                let line = Line::from(vec![
                                                    Span::styled(connector, style),
                                                    Span::styled(pane_name(pane).to_string(), style.fg(fg)),
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
