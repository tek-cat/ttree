use ratatui::{
    layout::{Constraint, Layout, Rect, Alignment},
    widgets::{Block, Borders, Paragraph, Clear},
    style::{Color, Style},
    Frame,
};
use crate::state::AppState;

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

pub fn render(frame: &mut Frame, state: &AppState, preview_data: &Option<crate::state::EmbeddedTerminal>, last_error: &Option<String>) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .split(frame.area());

    // Status Bar
    let status_bar = Paragraph::new(format!(
        " mux  [{:?}]  {} sessions  {} windows",
        state.input_mode,
        state.sessions.len(),
        state.windows.len()
    ))
    .block(Block::new().borders(Borders::BOTTOM));
    frame.render_widget(status_bar, chunks[0]);

    if let Some(err) = last_error {
        let error_p = Paragraph::new(format!("Error: {}", err))
            .style(Style::default().fg(Color::Red));
        frame.render_widget(error_p, Rect::new(0, 1, frame.area().width, 1));
    }

    let body_chunks = Layout::horizontal([
        Constraint::Percentage(28),
        Constraint::Fill(1),
    ])
    .split(chunks[1]);

    // Tree Sidebar
    let tree_block = Block::default()
        .title(" Sessions ")
        .borders(Borders::RIGHT);
    let tree_area = tree_block.inner(body_chunks[0]);
    frame.render_widget(tree_block, body_chunks[0]);
    frame.render_widget(TreeWidget::new(state), tree_area);

    // Preview / Relay Region
    let preview_block = Block::default()
        .title(" Preview ")
        .borders(Borders::ALL);
    let preview_area = preview_block.inner(body_chunks[1]);
    frame.render_widget(preview_block, body_chunks[1]);

    if let Some(terminal) = preview_data {
        if let Ok(parser) = terminal.parser.read() {
            let pseudo_term = tui_term::widget::PseudoTerminal::new(parser.screen());
            frame.render_widget(pseudo_term, preview_area);
        }
    }

    // Keybind Bar
    let keybind_bar = Paragraph::new("<Up/k> up  <Down/j> down  <Space> toggle  <Enter> attach  <?> help  <q> quit")
        .block(Block::new().borders(Borders::TOP));
    frame.render_widget(keybind_bar, chunks[2]);

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
        ].join("\n");

        let help_block = Paragraph::new(help_text)
            .block(Block::default().title(" Help ").borders(Borders::ALL))
            .alignment(Alignment::Left);

        frame.render_widget(Clear, area); // This clears out the background
        frame.render_widget(help_block, area);
    }
}

