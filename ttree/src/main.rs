use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration};
use std::os::unix::process::CommandExt;

mod state;
mod tmux_client;
mod ui;

use crate::state::AppState;
use crate::tmux_client::Tmux;

fn setup_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, DisableMouseCapture);
        default_hook(panic_info);
    }));
}

async fn sync_state(state: &mut AppState) -> Result<bool> {
    // 1. Fetch data from tmux
    let sessions_raw = Tmux::list_sessions().await.unwrap_or_default();
    let windows_raw = Tmux::list_windows().await.unwrap_or_default();
    let panes_raw = Tmux::list_panes().await.unwrap_or_default();

    // Store old IDs to check for changes
    let old_panes: std::collections::HashSet<String> = state.panes.keys().cloned().collect();

    // 2. Clear old state but preserve expansion/selection
    let expanded_sessions: std::collections::HashSet<String> = state.sessions.iter()
        .filter(|(_, s)| s.expanded)
        .map(|(id, _)| id.clone())
        .collect();
    let expanded_windows: std::collections::HashSet<String> = state.windows.iter()
        .filter(|(_, w)| w.expanded)
        .map(|(id, _)| id.clone())
        .collect();

    state.sessions.clear();
    state.windows.clear();
    state.panes.clear();

    // 3. Populate sessions
    for s in sessions_raw {
        let parts: Vec<&str> = s.split('\u{001f}').collect();
        if parts.len() >= 2 {
            let id = parts[0].to_string();
            state.sessions.insert(id.clone(), crate::state::Session {
                id: id.clone(),
                name: parts[1].to_string(),
                windows: Vec::new(),
                expanded: expanded_sessions.is_empty() || expanded_sessions.contains(&id),
            });
        }
    }

    // 4. Populate windows
    for w in windows_raw {
        let parts: Vec<&str> = w.split('\u{001f}').collect();
        if parts.len() >= 6 {
            let id = parts[0].to_string();
            let session_id = parts[1].to_string();
            state.windows.insert(id.clone(), crate::state::Window {
                id: id.clone(),
                session_id: session_id.clone(),
                name: parts[2].to_string(),
                panes: Vec::new(),
                active: parts[3] == "1",
                width: parts[4].parse().unwrap_or(80),
                height: parts[5].parse().unwrap_or(24),
                expanded: expanded_windows.is_empty() || expanded_windows.contains(&id),
            });
            if let Some(session) = state.sessions.get_mut(&session_id) {
                session.windows.push(id);
            }
        }
    }

    // 5. Populate panes
    for p in panes_raw {
        let parts: Vec<&str> = p.split('\u{001f}').collect();
        if parts.len() >= 9 {
            let id = parts[0].to_string();
            let window_id = parts[1].to_string();
            let left = parts[5].parse().unwrap_or(0);
            let top = parts[6].parse().unwrap_or(0);
            let width = parts[7].parse().unwrap_or(0);
            let height = parts[8].parse().unwrap_or(0);
            
            state.panes.insert(id.clone(), crate::state::Pane {
                id: id.clone(),
                window_id: window_id.clone(),
                title: parts[2].to_string(),
                current_command: parts[3].to_string(),
                active: parts[4] == "1",
                region: Some(ratatui::layout::Rect::new(left, top, width, height)),
            });
            if let Some(window) = state.windows.get_mut(&window_id) {
                window.panes.push(id);
            }
        }
    }
    
    let new_panes: std::collections::HashSet<String> = state.panes.keys().cloned().collect();
    let changed = old_panes != new_panes;

    // Auto-select first session if focus is empty
    if !state.sessions.is_empty() && state.focus.selected_id.is_none() {
        if let Some(first) = state.sessions.keys().next() {
            state.focus.selected_id = Some(first.clone());
        }
    }

    Ok(changed)
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_panic_hook();
    
    let mut state = AppState::default();
    let mut last_error: Option<String> = None;

    loop {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        
        if let Err(e) = sync_state(&mut state).await {
            last_error = Some(format!("Failed to sync tmux state: {}", e));
        }

        // Attempt to get preview for initially selected pane/window
        let mut last_preview_update = tokio::time::Instant::now() - Duration::from_secs(1);
        let mut last_sync_update = tokio::time::Instant::now();
        let mut preview_data: Option<crate::state::WindowPreview> = None;
        
        let mut action_attach = None;

        // Main Loop
        loop {
            // Periodic sync
            if last_sync_update.elapsed() > Duration::from_secs(2) {
                if let Ok(changed) = sync_state(&mut state).await {
                    if changed {
                        last_preview_update = tokio::time::Instant::now() - Duration::from_secs(1);
                    }
                }
                last_sync_update = tokio::time::Instant::now();
            }

            // Fetch preview
            if last_preview_update.elapsed() > Duration::from_millis(500) {
                let mut target_window_id = None;
                
                if let Some(sel) = &state.focus.selected_id {
                    if state.sessions.contains_key(sel) {
                        if let Some(s) = state.sessions.get(sel) {
                            for w_id in &s.windows {
                                if let Some(w) = state.windows.get(w_id) {
                                    if w.active {
                                        target_window_id = Some(w_id.clone());
                                        break;
                                    }
                                }
                            }
                            if target_window_id.is_none() {
                                target_window_id = s.windows.first().cloned();
                            }
                        }
                    } else if state.windows.contains_key(sel) {
                        target_window_id = Some(sel.clone());
                    } else if state.panes.contains_key(sel) {
                        if let Some(p) = state.panes.get(sel) {
                            target_window_id = Some(p.window_id.clone());
                        }
                    }
                }
// ... rest of preview fetching remains the same ...

                if let Some(w_id) = target_window_id {
                    if let Some(window) = state.windows.get(&w_id) {
                        let win_width = window.width;
                        let win_height = window.height;
                        
                        let mut set = tokio::task::JoinSet::new();
                        for p_id in &window.panes {
                            if let Some(pane) = state.panes.get(p_id) {
                                let p_id_clone = p_id.clone();
                                let region = pane.region.clone();
                                let active = pane.active;
                                set.spawn(async move {
                                    let content = Tmux::capture_pane(&p_id_clone).await.unwrap_or_default();
                                    crate::state::PanePreview {
                                        id: p_id_clone,
                                        region: region.unwrap_or_default(),
                                        content,
                                        active,
                                    }
                                });
                            }
                        }
                        
                        let mut panes = Vec::new();
                        while let Some(res) = set.join_next().await {
                            if let Ok(pane_preview) = res {
                                panes.push(pane_preview);
                            }
                        }

                        preview_data = Some(crate::state::WindowPreview {
                            window_id: w_id.clone(),
                            width: win_width,
                            height: win_height,
                            panes,
                        });
                    }
                }
                last_preview_update = tokio::time::Instant::now();
            }

            terminal.draw(|f| {
                ui::render(f, &state, &preview_data, &last_error);
            })?;

            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if state.show_help {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') => {
                                state.show_help = false;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                        KeyCode::Char('?') => {
                            state.show_help = true;
                        }
                        KeyCode::Enter => {

                            if let Some(sel) = &state.focus.selected_id {
                                action_attach = Some(sel.clone());
                                break;
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            state.move_selection_up();
                            last_preview_update = tokio::time::Instant::now() - Duration::from_secs(1); // Force immediate preview update
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            state.move_selection_down();
                            last_preview_update = tokio::time::Instant::now() - Duration::from_secs(1);
                        }
                        KeyCode::Char(' ') => {
                            state.toggle_expansion();
                        }
                        _ => {}
                    }
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        // Perform attach action outside of raw mode
        if let Some(target) = action_attach {
            // Identify target type. tmux attach-session -t takes session ID.
            let mut session_target = target.clone();
            if state.windows.contains_key(&target) {
                session_target = state.windows.get(&target).unwrap().session_id.clone();
            } else if state.panes.contains_key(&target) {
                let win_id = &state.panes.get(&target).unwrap().window_id;
                session_target = state.windows.get(win_id).unwrap().session_id.clone();
            }
            
            // Wait for the attach process to exit before continuing the loop
            let mut child = tokio::process::Command::new("tmux")
                .args(["attach-session", "-t", &session_target])
                .spawn()?;
                
            let _ = child.wait().await;
        } else {
            // User pressed q or Ctrl-C, exit the application entirely
            break;
        }
    }

    Ok(())
}
