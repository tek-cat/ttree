use crate::state::AppState;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

mod tree;
use tree::TreeWidget;

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

pub fn render(
    frame: &mut Frame,
    state: &mut AppState,
    preview_data: &Option<crate::state::EmbeddedTerminal>,
    last_error: &Option<String>,
) {
    let chunks = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).split(frame.area());

    if let Some(err) = last_error {
        let error_p =
            Paragraph::new(format!("Error: {}", err)).style(Style::default().fg(Color::Red));
        frame.render_widget(error_p, Rect::new(0, 0, frame.area().width, 1));
    }

    let sidebar_w = if state.sidebar_cols > 0 {
        state.sidebar_cols
    } else {
        (frame.area().width as f32 * 0.28) as u16
    };
    let body_chunks =
        Layout::horizontal([Constraint::Length(sidebar_w), Constraint::Fill(1)]).split(chunks[0]);

    // Tree Sidebar
    let tree_block = Block::default().title(" Sessions ").borders(Borders::RIGHT);
    let tree_area = tree_block.inner(body_chunks[0]);

    // Update scroll based on current tree area height
    state.update_scroll(tree_area.height as usize);

    frame.render_widget(tree_block, body_chunks[0]);
    frame.render_widget(TreeWidget::new(state), tree_area);

    // Preview / Relay Region
    let preview_block = Block::default();
    let preview_area = body_chunks[1];
    frame.render_widget(preview_block, body_chunks[1]);

    if let Some(terminal) = preview_data {
        if let Ok(parser) = terminal.parser.read() {
            let pseudo_term = tui_term::widget::PseudoTerminal::new(parser.screen());
            frame.render_widget(pseudo_term, preview_area);
        }
    }

    // Paint our own selection highlight over whatever the embedded terminal
    // just drew. Coordinates in PreviewSelection are vt100-local, so they get
    // offset by the preview area's origin and clipped to its rect.
    if let Some(sel) = &state.preview_selection {
        let (start, end) =
            if sel.anchor <= sel.head { (sel.anchor, sel.head) } else { (sel.head, sel.anchor) };
        let buf = frame.buffer_mut();
        let last_col = preview_area.width.saturating_sub(1);
        for vt_row in start.0..=end.0 {
            let row_start = if vt_row == start.0 { start.1 } else { 0 };
            let row_end = if vt_row == end.0 { end.1 } else { last_col };
            for vt_col in row_start..=row_end {
                let x = preview_area.x.saturating_add(vt_col);
                let y = preview_area.y.saturating_add(vt_row);
                if x < preview_area.x + preview_area.width
                    && y < preview_area.y + preview_area.height
                {
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.modifier |= Modifier::REVERSED;
                    }
                }
            }
        }
    }

    // Command Bar - contextual based on panel mode
    let (cmd_text, mode_label) = match state.focus.panel {
        crate::state::Panel::Tree => {
            ("SPACE toggle  ENTER preview  a attach  n new  r rename  ? help  Q quit", "TREE")
        }
        crate::state::Panel::Preview => {
            ("C-b d  back to tree    C-b <key>  pass prefix to tmux", "PREVIEW")
        }
    };

    // Bar background for the entire row (tmux status-style bg).
    let theme = &state.theme;
    let bg_rect =
        Paragraph::new("").style(Style::default().bg(theme.bar_bg)).block(Block::default());
    frame.render_widget(bg_rect, chunks[1]);

    // Mode label rendered as a tmux-style pill (window-status-current-style).
    let pill = format!(" {} ", mode_label);
    let mode_rect = Rect::new(chunks[1].x, chunks[1].y, pill.len() as u16, 1);
    let tree_label = Paragraph::new(pill.clone())
        .style(Style::default().bg(theme.pill_bg).fg(theme.pill_fg).add_modifier(Modifier::BOLD));
    frame.render_widget(tree_label, mode_rect);

    // Render commands starting after the pill (offset by pill width + 1 for space).
    let cmd_offset = pill.len() as u16 + 1;
    let cmd_area = Rect::new(
        chunks[1].x + cmd_offset,
        chunks[1].y,
        chunks[1].width.saturating_sub(cmd_offset),
        1,
    );
    let cmd_line = Paragraph::new(cmd_text)
        .style(Style::default().bg(theme.bar_bg).fg(theme.bar_fg))
        .alignment(Alignment::Left);
    frame.render_widget(cmd_line, cmd_area);

    // Rename Popup
    if let crate::state::InputMode::Renaming { input, .. } = &state.input_mode {
        let popup_width = 40u16;
        let popup_height = 3u16;
        let area = frame.area();
        let x = area.x + area.width.saturating_sub(popup_width) / 2;
        let y = area.y + area.height.saturating_sub(popup_height) / 2;
        let popup_area = Rect::new(x, y, popup_width.min(area.width), popup_height);
        let display = format!(" {}_", input);
        let popup = Paragraph::new(display)
            .block(Block::default().title(" Rename Session ").borders(Borders::ALL))
            .style(Style::default());
        frame.render_widget(Clear, popup_area);
        frame.render_widget(popup, popup_area);
    }

    // New Session Popup
    if let crate::state::InputMode::NewSession { input } = &state.input_mode {
        let popup_width = 40u16;
        let popup_height = 3u16;
        let area = frame.area();
        let x = area.x + area.width.saturating_sub(popup_width) / 2;
        let y = area.y + area.height.saturating_sub(popup_height) / 2;
        let popup_area = Rect::new(x, y, popup_width.min(area.width), popup_height);
        let display = format!(" {}_", input);
        let popup = Paragraph::new(display)
            .block(Block::default().title(" New Session Name ").borders(Borders::ALL))
            .style(Style::default());
        frame.render_widget(Clear, popup_area);
        frame.render_widget(popup, popup_area);
    }

    // Help Popup
    if state.show_help {
        let area = centered_rect(60, 70, frame.area());
        let help_text = vec![
            "Tree mode:",
            "  j / Down      move selection down",
            "  k / Up        move selection up",
            "  h / Left      collapse node / jump to parent",
            "  l / Right     expand node / jump to first child",
            "  Space         toggle expand/collapse",
            "  n             create new session",
            "  r             rename selected session",
            "  Enter         open preview panel",
            "  a             attach to selected session/window/pane",
            "  C-p           toggle tree / preview focus",
            "  ?             toggle this help",
            "  q / C-c       quit",
            "",
            "Preview mode:",
            "  C-b d         return to tree mode",
            "  C-b <key>     send tmux prefix + key",
            "  (all keys)    forwarded to embedded terminal",
            "",
            "Mouse:",
            "  Click tree    select item",
            "  Scroll tree   navigate up / down",
            "  Scroll preview  scroll embedded terminal",
            "  Drag separator  resize sidebar",
            "",
            "Press any key to close...",
        ]
        .join("\n");

        let help_block = Paragraph::new(help_text)
            .block(Block::default().title(" Help ").borders(Borders::ALL))
            .alignment(Alignment::Left);

        frame.render_widget(Clear, area);
        frame.render_widget(help_block, area);
    }
}
