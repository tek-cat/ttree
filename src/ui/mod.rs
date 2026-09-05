use crate::state::AppState;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
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

    // Tree Sidebar. The divider to the preview is a solid block column in the
    // tmux status-bar color, so it reads as part of the same chrome as the
    // command bar. Both the glyph and the cell behind it get that color: fonts
    // that render U+2588 with hairline gaps would otherwise show seams down it.
    let bar_bg = state.theme.bar_bg;
    let tree_block = Block::default()
        .title(" Sessions ")
        .borders(Borders::RIGHT)
        .border_set(symbols::border::Set { vertical_right: "█", ..symbols::border::PLAIN })
        .border_style(Style::default().fg(bar_bg).bg(bar_bg));
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
    } else if state.mirror_suppressed {
        // Say why the preview is empty. Mirroring our own session would draw
        // ttree inside ttree, so we decline rather than render a hall of mirrors.
        let note = Paragraph::new("ttree is running in this session, so it isn't mirrored here.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        let y = preview_area.y + preview_area.height / 2;
        frame.render_widget(note, Rect::new(preview_area.x, y, preview_area.width, 1));
    }

    // Command Bar - the filter prompt when one is being typed, otherwise the
    // key hints for the focused panel.
    let (cmd_text, mode_label) = match (&state.input_mode, &state.focus.panel) {
        // The bar doubles as the filter's input line rather than a popup: a
        // popup would cover the very rows the query is narrowing down.
        (crate::state::InputMode::Filtering { input }, _) => (format!("/{}█", input), "FILTER"),
        (_, crate::state::Panel::Tree) => (
            "SPACE toggle  ENTER preview  a attach  n new  r rename  ? help  Q quit".to_string(),
            "TREE",
        ),
        (_, crate::state::Panel::Preview) => {
            ("C-b d  back to tree    C-b <key>  pass prefix to tmux".to_string(), "PREVIEW")
        }
    };

    // A filter left applied after typing stops is why rows are missing, so it
    // keeps a quiet seat at the end of the hints.
    let filter_note = match &state.filter {
        Some(q)
            if !q.is_empty()
                && !matches!(state.input_mode, crate::state::InputMode::Filtering { .. }) =>
        {
            Some(format!("   /{}  (ESC clears)", q))
        }
        _ => None,
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
    let bar_style = Style::default().bg(theme.bar_bg).fg(theme.bar_fg);
    let mut cmd_spans = vec![Span::styled(cmd_text, bar_style)];
    if let Some(note) = filter_note {
        cmd_spans.push(Span::styled(note, bar_style.add_modifier(Modifier::DIM)));
    }
    let cmd_line =
        Paragraph::new(Line::from(cmd_spans)).style(bar_style).alignment(Alignment::Left);
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
            .block(Block::default().title(" Rename ").borders(Borders::ALL))
            .style(Style::default());
        frame.render_widget(Clear, popup_area);
        frame.render_widget(popup, popup_area);
    }

    // Confirm popup for anything destructive.
    if let crate::state::InputMode::Confirming { prompt, .. } = &state.input_mode {
        let popup_width = 52u16;
        let popup_height = 3u16;
        let area = frame.area();
        let x = area.x + area.width.saturating_sub(popup_width) / 2;
        let y = area.y + area.height.saturating_sub(popup_height) / 2;
        let popup_area = Rect::new(x, y, popup_width.min(area.width), popup_height);
        let popup = Paragraph::new(format!(" {}  [y/N]", prompt))
            .block(Block::default().title(" Confirm ").borders(Borders::ALL))
            .style(Style::default().fg(theme.icon));
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
            "  c             create window in selected session",
            "  % / \"         split selected pane right / below",
            "  r             rename selected session or window",
            "  x             kill selected (asks first)",
            "  /             filter the tree (ESC clears)",
            "  Enter         open preview panel",
            "  a             attach to selected session/window/pane",
            "  C-p           toggle tree / preview focus",
            "  ?             toggle this help",
            "  q / C-c       quit",
            "",
            "Preview mode:",
            "  <prefix> d    return to tree mode (prefix follows tmux)",
            "  <prefix> <key>  send tmux prefix + key",
            "  <prefix> <prefix>  send one prefix through, for nested tmux",
            "  (all keys)    forwarded to embedded terminal",
            "",
            "Mouse:",
            "  Click tree      select item",
            "  Scroll tree     navigate up / down",
            "  Drag separator  resize sidebar",
            "  In preview      passed through to tmux: click a pane,",
            "                  drag to select, scroll the scrollback",
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
