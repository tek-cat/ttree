use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration, sync::{Arc, RwLock}};
use portable_pty::{CommandBuilder, native_pty_system, PtySize};

mod state;
mod tmux_client;
mod ui;

use crate::state::{AppState, Panel, EmbeddedTerminal};
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
    let (sessions_raw, windows_raw, panes_raw) = tokio::join!(
        Tmux::list_sessions(),
        Tmux::list_windows(),
        Tmux::list_panes()
    );

    let sessions_raw = sessions_raw.unwrap_or_default();
    let windows_raw = windows_raw.unwrap_or_default();
    let panes_raw = panes_raw.unwrap_or_default();

    let old_panes: std::collections::HashSet<String> = state.panes.keys().cloned().collect();

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

    if let Some(sel) = &state.focus.selected_id {
        if !state.sessions.contains_key(sel) 
            && !state.windows.contains_key(sel) 
            && !state.panes.contains_key(sel) {
            state.focus.selected_id = None;
            if let Some(first) = state.sessions.keys().next() {
                state.focus.selected_id = Some(first.clone());
            }
        }
    }

    if state.focus.selected_id.is_none() {
        let list = state.get_flat_list(&state.focus.nav_mode);
        if !list.is_empty() {
            state.focus.selected_id = Some(list[0].clone());
        }
    }

    Ok(changed)
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_panic_hook();

    loop {
        match run_app().await {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
}

async fn run_app() -> Result<()> {
    let mut state = AppState::default();
    let last_error: Option<String> = None;
    let mut active_terminal: Option<EmbeddedTerminal> = None;
    let mut pty_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut last_selected_id: Option<String> = None;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut last_sync_update = tokio::time::Instant::now();
    let action_attach: Option<String>;

    let _ = sync_state(&mut state).await;

    let mut last_terminal_size = terminal.size().unwrap_or(ratatui::layout::Size::new(80, 24));
    loop {
        if tokio::time::Instant::now() > last_sync_update + Duration::from_millis(200) {
            let _ = sync_state(&mut state).await;
            last_sync_update = tokio::time::Instant::now();
        }

        let current_size = terminal.size().unwrap_or(last_terminal_size);
        if current_size != last_terminal_size {
            if let Some(term) = &mut active_terminal {
                let sidebar_width = (current_size.width as f32 * 0.28) as u16;
                let cols = current_size.width.saturating_sub(sidebar_width).saturating_sub(2);
                let rows = current_size.height.saturating_sub(4);
                
                if let Ok(master) = term.pty_master.lock() {
                    let _ = master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
                }
            }
            last_terminal_size = current_size;
        }

        if state.focus.selected_id != last_selected_id {
            state.focus.enable_scrolling = true;

            if let Some(target_id) = &state.focus.selected_id {
                if let Some(term) = &mut active_terminal {
                    let target_id_clone = target_id.clone();
                    term.target_id = target_id.clone();
                    
                    if let Some(pid) = term.pty_pid {
                        tokio::spawn(async move {
                            if let Ok(output) = tokio::process::Command::new("tmux")
                                .args(["list-clients", "-F", "#{client_pid} #{client_tty}"])
                                .output()
                                .await
                            {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let mut client_tty = None;
                                for line in stdout.lines() {
                                    let parts: Vec<&str> = line.split_whitespace().collect();
                                    if parts.len() == 2 && parts[0] == pid.to_string() {
                                        client_tty = Some(parts[1].to_string());
                                        break;
                                    }
                                }
                                
                                if let Some(tty) = client_tty {
                                    let mut cmd = tokio::process::Command::new("tmux");
                                    if target_id_clone.starts_with('%') {
                                        cmd.args(["select-pane", "-t", &target_id_clone, ";"]);
                                    }
                                    cmd.args(["switch-client", "-c", &tty, "-t", &target_id_clone]);
                                    let _ = cmd.output().await;
                                }
                            }
                        });
                    }
                } else {
                    let size = terminal.size().unwrap_or(ratatui::layout::Size::new(80, 24));
                    let sidebar_width = (size.width as f32 * 0.28) as u16;
                    let cols = size.width.saturating_sub(sidebar_width).saturating_sub(2);
                    let rows = size.height.saturating_sub(4);

                    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                    let parser = Arc::new(RwLock::new(vt100::Parser::new(rows, cols, 0)));
                    
                    let pty_system = native_pty_system();
                    if let Ok(pair) = pty_system.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }) {
                        let mut cmd = CommandBuilder::new("tmux");
                        let target_id_clone = target_id.clone();
                        if target_id_clone.starts_with('%') {
                            cmd.args(["select-pane", "-t", &target_id_clone, ";", "attach-session", "-t", &target_id_clone]);
                        } else {
                            cmd.args(["attach", "-t", &target_id_clone]);
                        }
                        
                        if let Ok(child) = pair.slave.spawn_command(cmd) {
                            let pid = child.process_id();
                            drop(pair.slave);
                            
                            let mut reader = pair.master.try_clone_reader().unwrap();
                            let mut writer = pair.master.take_writer().unwrap();

                            active_terminal = Some(EmbeddedTerminal {
                                parser: parser.clone(),
                                pty_writer: tx,
                                target_id: target_id.clone(),
                                pty_pid: pid,
                                pty_master: std::sync::Arc::new(std::sync::Mutex::new(pair.master)),
                            });
                            let mut receiver = rx;
                            let parser_clone = parser.clone();

                            let reader_task = tokio::task::spawn_blocking(move || {
                                let mut buf = [0u8; 4096];
                                loop {
                                    match std::io::Read::read(&mut reader, &mut buf) {
                                        Ok(n) if n > 0 => {
                                            if let Ok(mut p) = parser_clone.write() {
                                                p.process(&buf[..n]);
                                            }
                                        }
                                        _ => break,
                                    }
                                }
                            });

                            let writer_task = tokio::task::spawn_blocking(move || {
                                while let Some(data) = receiver.blocking_recv() {
                                    if std::io::Write::write_all(&mut writer, &data).is_err() {
                                        break;
                                    }
                                }
                            });

                            let child_arc = std::sync::Arc::new(std::sync::Mutex::new(child));
                            let handle = tokio::spawn(async move {
                                loop {
                                    let status = {
                                        if let Ok(mut c) = child_arc.lock() {
                                            c.try_wait()
                                        } else {
                                            break;
                                        }
                                    };
                                    match status {
                                        Ok(Some(_)) => break,
                                        Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
                                        Err(_) => break,
                                    }
                                }
                                reader_task.abort();
                                writer_task.abort();
                            });
                            pty_task = Some(handle);
                        }
                    }
                }
            }
            last_selected_id = state.focus.selected_id.clone();
        }

        // Clean up pty_task if it finished
        let task_finished = if let Some(task) = &pty_task {
            task.is_finished()
        } else {
            false
        };
        if task_finished {
            pty_task = None;
            active_terminal = None;
            // Switch back to Tree panel if the terminal exits
            state.focus.panel = Panel::Tree;
        }

        terminal.draw(|f| {
            ui::render(f, &mut state, &active_terminal, &last_error);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    state.focus.panel = match state.focus.panel {
                        Panel::Tree => Panel::Preview,
                        Panel::Preview => Panel::Tree,
                    };
                    continue;
                }

                if state.focus.panel == Panel::Tree {
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
                        KeyCode::Char('q') => {
                            disable_raw_mode()?;
                            execute!(
                                io::stdout(),
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;
                            std::process::exit(0);
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            disable_raw_mode()?;
                            execute!(
                                io::stdout(),
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;
                            std::process::exit(0);
                        }
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
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            state.move_selection_down();
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            state.switch_nav_left();
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            state.switch_nav_right();
                        }
                        KeyCode::Char(' ') => {
                            state.toggle_expansion();
                        }
                        _ => {}
                    }
                } else if state.focus.panel == Panel::Preview {
                    if let Some(term) = &active_terminal {
                        let mut bytes = Vec::new();
                        match key.code {
                            KeyCode::Char(c) => {
                                if key.modifiers.contains(KeyModifiers::CONTROL) {
                                    if c >= 'a' && c <= 'z' {
                                        bytes.push(c as u8 - b'a' + 1);
                                    } else if (b'@'..=b'_').contains(&(c as u8)) {
                                        bytes.push(c as u8 - b'@');
                                    } else if c == ' ' {
                                        bytes.push(0);
                                    }
                                } else if key.modifiers.contains(KeyModifiers::ALT) {
                                    bytes.push(27);
                                    bytes.extend_from_slice(c.to_string().as_bytes());
                                } else {
                                    bytes.extend_from_slice(c.to_string().as_bytes());
                                }
                            }
                            KeyCode::Enter => bytes.push(b'\r'),
                            KeyCode::Esc => bytes.push(27),
                            KeyCode::Backspace => bytes.push(127),
                            KeyCode::Tab => bytes.push(b'\t'),
                            KeyCode::Up => bytes.extend_from_slice(b"\x1b[A"),
                            KeyCode::Down => bytes.extend_from_slice(b"\x1b[B"),
                            KeyCode::Right => bytes.extend_from_slice(b"\x1b[C"),
                            KeyCode::Left => bytes.extend_from_slice(b"\x1b[D"),
                            KeyCode::Home => bytes.extend_from_slice(b"\x1b[H"),
                            KeyCode::End => bytes.extend_from_slice(b"\x1b[F"),
                            KeyCode::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
                            KeyCode::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
                            KeyCode::Delete => bytes.extend_from_slice(b"\x1b[3~"),
                            KeyCode::F(n) => {
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
                            }
                            _ => {}
                        }
                        if !bytes.is_empty() {
                            let _ = term.pty_writer.send(bytes);
                        }
                    }
                }
            }
        }
    }

    if let Some(task) = pty_task {
        task.abort();
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Some(target) = action_attach {
        let mut session_target = target.clone();
        let use_pane = target.starts_with('%');
        
        if state.windows.contains_key(&target) {
            session_target = state.windows.get(&target).unwrap().session_id.clone();
        } else if state.panes.contains_key(&target) {
            let win_id = &state.panes.get(&target).unwrap().window_id;
            session_target = state.windows.get(win_id).unwrap().session_id.clone();
        }
        
        if use_pane {
            let mut child = tokio::process::Command::new("tmux")
                .args(["select-pane", "-t", &target, ";", "attach-session", "-t", &session_target])
                .spawn()?;
            let _ = child.wait().await;
        } else {
            let mut child = tokio::process::Command::new("tmux")
                .args(["attach-session", "-t", &session_target])
                .spawn()?;
            let _ = child.wait().await;
        }
    }

    Ok(())
}
