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
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .split(frame.area());

    // Status Bar
    let status_bar = Paragraph::new(format!(
        " mux  [{:?}]  Nav: {:?}  {} sessions  {} windows",
        state.input_mode,
        state.focus.nav_mode,
        state.sessions.len(),
        state.windows.len()
    ))
    .block(Block::new().borders(Borders::BOTTOM));
    frame.render_widget(status_bar, chunks[0]);

    if let Some(err) = last_error {
        let error_p =
            Paragraph::new(format!("Error: {}", err)).style(Style::default().fg(Color::Red));
        frame.render_widget(error_p, Rect::new(0, 1, frame.area().width, 1));
    }

    let body_chunks =
        Layout::horizontal([Constraint::Percentage(28), Constraint::Fill(1)]).split(chunks[1]);

    // Tree Sidebar
    let tree_block = Block::default().title(" Sessions ").borders(Borders::RIGHT);
    let tree_area = tree_block.inner(body_chunks[0]);

    // Update scroll based on current tree area height
    state.update_scroll(tree_area.height as usize);

    frame.render_widget(tree_block, body_chunks[0]);
    frame.render_widget(TreeWidget::new(state), tree_area);

    // Preview / Relay Region
    let preview_block = Block::default().title(" Terminal ").borders(Borders::ALL);
    let preview_area = preview_block.inner(body_chunks[1]);
    frame.render_widget(preview_block, body_chunks[1]);

    if let Some(terminal) = preview_data {
        if let Ok(parser) = terminal.parser.read() {
            let pseudo_term = tui_term::widget::PseudoTerminal::new(parser.screen());
            frame.render_widget(pseudo_term, preview_area);
        }
    }

    // Command Bar - contextual based on panel mode
    let (cmd_text, mode_label) = match state.focus.panel {
        crate::state::Panel::Tree => (
            "UP up  DOWN down  LEFT lt  RIGHT rt  SPACE toggle  ENTER attach  ? help  Q quit",
            "TREE",
        ),
        crate::state::Panel::Preview => (
            "ARROWS term  TAB esc  ENTER enter  CTRL-P tree  Q quit",
            "PREVIEW",
        ),
    };

    // Green background for entire bar
    let bg_rect = Paragraph::new("")
        .style(Style::default().bg(Color::Green))
        .block(Block::default());
    frame.render_widget(bg_rect, chunks[2]);

    // Render mode label
    let mode_bg = Color::Green;
    let mode_rect = Rect::new(chunks[2].x, chunks[2].y, mode_label.len() as u16 + 1, 1);

    let tree_label = Paragraph::new(mode_label).style(
        Style::default()
            .bg(mode_bg)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(tree_label, mode_rect);

    // Render commands starting after the mode label (offset by mode label width + 1 for space)
    let cmd_offset = mode_label.len() as u16 + 1;
    let cmd_area = Rect::new(
        chunks[2].x + cmd_offset,
        chunks[2].y,
        chunks[2].width.saturating_sub(cmd_offset),
        1,
    );
    let cmd_line = Paragraph::new(cmd_text)
        .style(Style::default().fg(Color::Black))
        .alignment(Alignment::Left);
    frame.render_widget(cmd_line, cmd_area);

    // Help Popup
    if state.show_help {
        let area = centered_rect(50, 50, frame.area());
        let help_text = vec![
            "Navigation:",
            "  Up / k    - Move selection up",
            "  Down / j  - Move selection down",
            "  Space     - Toggle expand/collapse session or window",
            "  Enter     - Attach to selected session/window/pane",
            "  ?         - Toggle this help menu",
            "  q         - Quit",
            "",
            "Press any key to close...",
        ]
        .join("\n");

        let help_block = Paragraph::new(help_text)
            .block(Block::default().title(" Help ").borders(Borders::ALL))
            .alignment(Alignment::Left);

        frame.render_widget(Clear, area); // This clears out the background
        frame.render_widget(help_block, area);
    }
}
