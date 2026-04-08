use crate::state::AppState;
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

impl<'a> Widget for TreeWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;
        for session in self.state.sessions.values() {
            if y >= area.bottom() {
                break;
            }

            let is_selected = Some(&session.id) == self.state.focus.selected_id.as_ref();
            let bg_color = if is_selected {
                Color::DarkGray
            } else {
                Color::Reset
            };

            let icon = if session.expanded { "▼" } else { "▶" };
            let mut style = Style::default().bg(bg_color);
            if is_selected {
                style = style.add_modifier(Modifier::BOLD);
            }

            let line = Line::from(vec![
                Span::styled(format!("{} ", icon), style.fg(Color::Yellow)),
                Span::styled(session.name.clone(), style),
                Span::styled(
                    format!(" ({})", session.windows.len()),
                    style.fg(Color::Gray),
                ),
            ]);
            buf.set_line(area.x, y, &line, area.width);
            y += 1;

            if session.expanded {
                for window_id in &session.windows {
                    if y >= area.bottom() {
                        break;
                    }
                    if let Some(window) = self.state.windows.get(window_id) {
                        let is_win_selected =
                            Some(&window.id) == self.state.focus.selected_id.as_ref();
                        let win_bg_color = if is_win_selected {
                            Color::DarkGray
                        } else {
                            Color::Reset
                        };

                        let active_style = if window.active {
                            Style::default().bg(win_bg_color).fg(Color::Cyan)
                        } else {
                            Style::default().bg(win_bg_color)
                        };

                        let win_icon = if window.panes.is_empty() {
                            " "
                        } else if window.expanded {
                            "▼"
                        } else {
                            "▶"
                        };
                        let line = Line::from(vec![
                            Span::styled(
                                format!("  {} ", win_icon),
                                Style::default().bg(win_bg_color).fg(Color::Yellow),
                            ),
                            Span::styled(window.name.clone(), active_style),
                        ]);
                        buf.set_line(area.x, y, &line, area.width);
                        y += 1;

                        if window.expanded {
                            let num_panes = window.panes.len();
                            for (idx, pane_id) in window.panes.iter().enumerate() {
                                if y >= area.bottom() {
                                    break;
                                }
                                if let Some(pane) = self.state.panes.get(pane_id) {
                                    let is_pane_selected =
                                        Some(&pane.id) == self.state.focus.selected_id.as_ref();
                                    let pane_bg_color = if is_pane_selected {
                                        Color::DarkGray
                                    } else {
                                        Color::Reset
                                    };

                                    let pane_active_style = if pane.active {
                                        Style::default().bg(pane_bg_color).fg(Color::Cyan)
                                    } else {
                                        Style::default().bg(pane_bg_color)
                                    };

                                    let pane_name = if !pane.current_command.is_empty() {
                                        pane.current_command.clone()
                                    } else {
                                        pane.title.clone()
                                    };

                                    let layout_hint = if let Some(region) = &pane.region {
                                        if region.width == 0 || region.height == 0 {
                                            String::new()
                                        } else if region.x == 0 && region.y == 0 {
                                            " [top-left]".to_string()
                                        } else if region.x > 0 && region.y == 0 {
                                            " [top-right]".to_string()
                                        } else if region.x == 0 && region.y > 0 {
                                            " [bottom-left]".to_string()
                                        } else {
                                            " [bottom-right]".to_string()
                                        }
                                    } else {
                                        String::new()
                                    };

                                    let connector = if idx == num_panes - 1 {
                                        "    └─ "
                                    } else {
                                        "    ├─ "
                                    };
                                    let line = Line::from(vec![
                                        Span::styled(connector, Style::default().bg(pane_bg_color)),
                                        Span::styled(pane_name, pane_active_style),
                                        Span::styled(
                                            layout_hint,
                                            Style::default().bg(pane_bg_color).fg(Color::DarkGray),
                                        ),
                                    ]);
                                    buf.set_line(area.x, y, &line, area.width);
                                    y += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
