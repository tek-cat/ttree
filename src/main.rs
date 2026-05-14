use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
        KeyboardEnhancementFlags, MouseButton, MouseEventKind, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
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
        let _ = execute!(
            stdout,
            PopKeyboardEnhancementFlags,
            LeaveAlternateScreen,
            DisableMouseCapture
        );
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

    let mut expanded_ids: std::collections::HashSet<String> = state.sessions.iter()
        .filter(|(_, s)| s.expanded)
        .map(|(id, _)| id.clone())
        .collect();
    expanded_ids.extend(state.windows.iter()
        .filter(|(_, w)| w.expanded)
        .map(|(id, _)| id.clone()));
    
    let is_initial_load = state.sessions.is_empty();

    // If we just loaded from disk and have no sessions yet, use the pre-loaded expanded_ids
    if expanded_ids.is_empty() && !state.expanded_ids.is_empty() {
        expanded_ids = state.expanded_ids.clone();
    }

    state.sessions.clear();
    state.windows.clear();
    state.panes.clear();

    for s in sessions_raw {
        let parts: Vec<&str> = s.split('\u{001f}').collect();
        if parts.len() >= 4 {
            let id = parts[0].to_string();
            let expanded = if is_initial_load && state.expanded_ids.is_empty() {
                true // Default to expanded on very first run
            } else {
                expanded_ids.contains(&id)
            };

            state.sessions.insert(id.clone(), crate::state::Session {
                id: id.clone(),
                name: parts[1].to_string(),
                windows: Vec::new(),
                expanded,
                last_attached: parts[2].parse().unwrap_or(0),
                attached: parts[3] == "1",
            });
        }
    }

    for w in windows_raw {
        let parts: Vec<&str> = w.split('\u{001f}').collect();
        if parts.len() >= 6 {
            let id = parts[0].to_string();
            let session_id = parts[1].to_string();
            
            let expanded = if is_initial_load && state.expanded_ids.is_empty() {
                true // Default to expanded on very first run
            } else {
                expanded_ids.contains(&id)
            };

            state.windows.insert(id.clone(), crate::state::Window {
                id: id.clone(),
                session_id: session_id.clone(),
                name: parts[2].to_string(),
                panes: Vec::new(),
                active: parts[3] == "1",
                width: parts[4].parse().unwrap_or(80),
                height: parts[5].parse().unwrap_or(24),
                expanded,
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

    let current_list = state.get_dynamic_visible_items();

    if let Some(sel) = &state.focus.selected_id {
        if !current_list.contains(sel) {
            state.focus.selected_id = None;
        }
    }

    if state.focus.selected_id.is_none() && !current_list.is_empty() {
        // Try to find the most appropriate active pane to focus
        let mut active_pane = if let Ok(current_pane) = std::env::var("TMUX_PANE") {
            state.panes.get(&current_pane).cloned()
        } else { None };

        // 2. Try the last target we attached to in this ttree session
        if active_pane.is_none() {
            if let Some(target_id) = &state.last_target_id {
                if let Some(p) = state.panes.get(target_id) {
                    active_pane = Some(p.clone());
                } else if let Some(w) = state.windows.get(target_id) {
                    active_pane = w.panes.iter()
                        .find_map(|pid| state.panes.get(pid).filter(|p| p.active))
                        .cloned();
                } else if let Some(s) = state.sessions.get(target_id) {
                    active_pane = s.windows.iter()
                        .find_map(|wid| {
                            state.windows.get(wid)
                                .filter(|w| w.active)
                                .and_then(|w| w.panes.iter().find_map(|pid| state.panes.get(pid).filter(|p| p.active)))
                        })
                        .cloned();
                }
            }
        }

        // 3. Fallback to sorting by last_attached
        if active_pane.is_none() {
            let mut active_panes: Vec<_> = state.panes.values()
                .filter(|p| {
                    p.active && state.windows.get(&p.window_id).map(|w| w.active).unwrap_or(false)
                })
                .collect();
            
            // Sort by session activity to pick the most recently active one
            active_panes.sort_by(|a, b| {
                let sess_a = state.windows.get(&a.window_id).and_then(|w| state.sessions.get(&w.session_id));
                let sess_b = state.windows.get(&b.window_id).and_then(|w| state.sessions.get(&w.session_id));
                
                let time_a = sess_a.map(|s| s.last_attached).unwrap_or(0);
                let time_b = sess_b.map(|s| s.last_attached).unwrap_or(0);
                
                if time_a != time_b {
                    return time_b.cmp(&time_a); // Newest first
                }

                // Tie-breaker: attached sessions
                let att_a = sess_a.map(|s| s.attached).unwrap_or(false);
                let att_b = sess_b.map(|s| s.attached).unwrap_or(false);
                att_b.cmp(&att_a)
            });

            active_pane = active_panes.first().cloned().cloned();
        }

        if let Some(pane) = active_pane {
            state.focus.selected_id = Some(pane.id.clone());
            state.focus.nav_mode = crate::state::NavMode::Pane;
            // Ensure path to active pane is expanded
            if let Some(window) = state.windows.get_mut(&pane.window_id) {
                window.expanded = true;
                if let Some(session) = state.sessions.get_mut(&window.session_id) {
                    session.expanded = true;
                }
            }
        } else {
            state.focus.selected_id = Some(current_list[0].clone());
        }
    }

    Ok(changed)
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_panic_hook();

    let mut state = AppState::load_from_disk();
    loop {
        match run_app(&mut state).await {
            Ok(()) => {
                let last_target = state.last_target_id.clone();
                // Reload state from disk to get the latest focus/expanded state
                state = AppState::load_from_disk();
                state.last_target_id = last_target;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
}

fn encode_mouse(mouse: &crossterm::event::MouseEvent, x_offset: u16, y_offset: u16) -> Option<Vec<u8>> {
    use crossterm::event::{MouseButton, MouseEventKind};

    let col = mouse.column.saturating_sub(x_offset) + 1;
    let row = mouse.row.saturating_sub(y_offset) + 1;

    let (base_button, release) = match mouse.kind {
        MouseEventKind::Down(btn) => (match btn {
            MouseButton::Left => 0u32,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        }, false),
        MouseEventKind::Up(btn) => (match btn {
            MouseButton::Left => 0u32,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        }, true),
        MouseEventKind::Drag(btn) => (match btn {
            MouseButton::Left => 32u32,
            MouseButton::Middle => 33,
            MouseButton::Right => 34,
        }, false),
        MouseEventKind::Moved => (35, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        _ => return None,
    };

    let mut button = base_button;
    if mouse.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) { button += 4; }
    if mouse.modifiers.contains(crossterm::event::KeyModifiers::ALT)   { button += 8; }
    if mouse.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) { button += 16; }

    let suffix = if release { 'm' } else { 'M' };
    Some(format!("\x1b[<{};{};{}{}", button, col, row, suffix).into_bytes())
}

fn modifier_code(mods: KeyModifiers) -> u8 {
    let mut m = 1u8;
    if mods.contains(KeyModifiers::SHIFT) { m += 1; }
    if mods.contains(KeyModifiers::ALT) { m += 2; }
    if mods.contains(KeyModifiers::CONTROL) { m += 4; }
    m
}

fn csi_letter(letter: char, m: u8, bytes: &mut Vec<u8>) {
    if m == 1 {
        bytes.extend_from_slice(format!("\x1b[{}", letter).as_bytes());
    } else {
        bytes.extend_from_slice(format!("\x1b[1;{}{}", m, letter).as_bytes());
    }
}

fn csi_tilde(n: u8, m: u8, bytes: &mut Vec<u8>) {
    if m == 1 {
        bytes.extend_from_slice(format!("\x1b[{}~", n).as_bytes());
    } else {
        bytes.extend_from_slice(format!("\x1b[{};{}~", n, m).as_bytes());
    }
}

fn encode_key(key: &crossterm::event::KeyEvent, bytes: &mut Vec<u8>) {
    use crossterm::event::KeyCode;
    let mods = key.modifiers;
    let m = modifier_code(mods);
    let alt = mods.contains(KeyModifiers::ALT);
    let ctrl = mods.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Char(c) => {
            if alt { bytes.push(0x1b); }
            if ctrl {
                let lc = c.to_ascii_lowercase();
                if lc.is_ascii_lowercase() {
                    bytes.push(lc as u8 - b'a' + 1);
                } else if (b'@'..=b'_').contains(&(c as u8)) {
                    bytes.push(c as u8 - b'@');
                } else if c == ' ' {
                    bytes.push(0);
                } else if c == '?' {
                    bytes.push(127);
                } else if c == '/' {
                    bytes.push(31);
                } else {
                    bytes.extend_from_slice(c.to_string().as_bytes());
                }
            } else {
                bytes.extend_from_slice(c.to_string().as_bytes());
            }
        }
        KeyCode::Enter => {
            if mods.is_empty() {
                bytes.push(b'\r');
            } else if mods == KeyModifiers::ALT {
                bytes.push(0x1b);
                bytes.push(b'\r');
            } else {
                bytes.extend_from_slice(format!("\x1b[13;{}u", m).as_bytes());
            }
        }
        KeyCode::Esc => {
            if mods.is_empty() {
                bytes.push(27);
            } else if mods == KeyModifiers::ALT {
                bytes.push(0x1b);
                bytes.push(27);
            } else {
                bytes.extend_from_slice(format!("\x1b[27;{}u", m).as_bytes());
            }
        }
        KeyCode::Backspace => {
            if mods.contains(KeyModifiers::SHIFT) {
                bytes.extend_from_slice(format!("\x1b[127;{}u", m).as_bytes());
            } else {
                if alt { bytes.push(0x1b); }
                if ctrl { bytes.push(0x08); } else { bytes.push(127); }
            }
        }
        KeyCode::Tab => {
            if mods.is_empty() {
                bytes.push(b'\t');
            } else if mods == KeyModifiers::SHIFT {
                bytes.extend_from_slice(b"\x1b[Z");
            } else if mods == KeyModifiers::ALT {
                bytes.push(0x1b);
                bytes.push(b'\t');
            } else {
                bytes.extend_from_slice(format!("\x1b[9;{}u", m).as_bytes());
            }
        }
        KeyCode::BackTab => {
            if mods.is_empty() || mods == KeyModifiers::SHIFT {
                bytes.extend_from_slice(b"\x1b[Z");
            } else {
                let mut bm = m;
                if !mods.contains(KeyModifiers::SHIFT) { bm += 1; }
                bytes.extend_from_slice(format!("\x1b[9;{}u", bm).as_bytes());
            }
        }
        KeyCode::Up => csi_letter('A', m, bytes),
        KeyCode::Down => csi_letter('B', m, bytes),
        KeyCode::Right => csi_letter('C', m, bytes),
        KeyCode::Left => csi_letter('D', m, bytes),
        KeyCode::Home => csi_letter('H', m, bytes),
        KeyCode::End => csi_letter('F', m, bytes),
        KeyCode::Insert => csi_tilde(2, m, bytes),
        KeyCode::Delete => csi_tilde(3, m, bytes),
        KeyCode::PageUp => csi_tilde(5, m, bytes),
        KeyCode::PageDown => csi_tilde(6, m, bytes),
        KeyCode::F(n) => {
            if m == 1 {
                let s = match n {
                    1 => "\x1bOP", 2 => "\x1bOQ", 3 => "\x1bOR", 4 => "\x1bOS",
                    5 => "\x1b[15~", 6 => "\x1b[17~", 7 => "\x1b[18~", 8 => "\x1b[19~",
                    9 => "\x1b[20~", 10 => "\x1b[21~", 11 => "\x1b[23~", 12 => "\x1b[24~",
                    _ => "",
                };
                bytes.extend_from_slice(s.as_bytes());
            } else {
                let s = match n {
                    1 => format!("\x1b[1;{}P", m),
                    2 => format!("\x1b[1;{}Q", m),
                    3 => format!("\x1b[1;{}R", m),
                    4 => format!("\x1b[1;{}S", m),
                    5 => format!("\x1b[15;{}~", m),
                    6 => format!("\x1b[17;{}~", m),
                    7 => format!("\x1b[18;{}~", m),
                    8 => format!("\x1b[19;{}~", m),
                    9 => format!("\x1b[20;{}~", m),
                    10 => format!("\x1b[21;{}~", m),
                    11 => format!("\x1b[23;{}~", m),
                    12 => format!("\x1b[24;{}~", m),
                    _ => String::new(),
                };
                bytes.extend_from_slice(s.as_bytes());
            }
        }
        _ => {}
    }
}

async fn run_app(state: &mut AppState) -> Result<()> {
    let last_error: Option<String> = None;
    let mut active_terminal: Option<EmbeddedTerminal> = None;
    let mut pty_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut last_selected_id: Option<String> = None;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut last_sync_update = tokio::time::Instant::now();
    let action_attach: Option<String>;

    // If we're inside tmux, we want to start by focusing the active pane.
    // Otherwise, we prefer to keep our previous selection (e.g. after a detach)
    if std::env::var("TMUX").is_ok() {
        state.focus.selected_id = None;
    }
    let _ = sync_state(state).await;

    let mut last_terminal_size = terminal.size().unwrap_or(ratatui::layout::Size::new(80, 24));
    let mut last_sidebar_cols: u16 = 0;
    let mut dragging_separator = false;
    let mut prefix_pending = false;
    let mut current_switch_task: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        if tokio::time::Instant::now() > last_sync_update + Duration::from_millis(200) {
            let _ = sync_state(state).await;
            last_sync_update = tokio::time::Instant::now();

            // In preview mode, sync tree selection to whichever session the embedded client is on
            if state.focus.panel == Panel::Preview {
                if let Some(term) = &active_terminal {
                    if let Some(pid) = term.pty_pid {
                        if let Ok(output) = tokio::process::Command::new("tmux")
                            .args(["list-clients", "-F", "#{client_pid} #{session_id} #{pane_id}"])
                            .output()
                            .await
                        {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            for line in stdout.lines() {
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if parts.len() == 3 && parts[0] == pid.to_string() {
                                    let current_session = parts[1].to_string();
                                    let current_pane = parts[2].to_string();
                                    let visible = state.get_dynamic_visible_items();
                                    // Prefer exact pane match; fall back to first item in session
                                    let target = if visible.contains(&current_pane) {
                                        Some(current_pane)
                                    } else {
                                        visible.into_iter().find(|id| {
                                            if id == &current_session { return true; }
                                            if let Some(w) = state.windows.get(id) {
                                                return w.session_id == current_session;
                                            }
                                            if let Some(p) = state.panes.get(id) {
                                                return state.windows.get(&p.window_id)
                                                    .map(|w| w.session_id == current_session)
                                                    .unwrap_or(false);
                                            }
                                            false
                                        })
                                    };
                                    if let Some(id) = target {
                                        if state.focus.selected_id.as_deref() != Some(&id) {
                                            state.focus.selected_id = Some(id);
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        let current_size = terminal.size().unwrap_or(last_terminal_size);

        // Initialize sidebar_cols from default percentage if not yet set
        if state.sidebar_cols == 0 {
            state.sidebar_cols = ((current_size.width as f32 * 0.28) as u16).max(8);
        }

        let sidebar_changed = state.sidebar_cols != last_sidebar_cols;
        if current_size != last_terminal_size || sidebar_changed {
            if let Some(term) = &mut active_terminal {
                let cols = current_size.width.saturating_sub(state.sidebar_cols);
                let rows = current_size.height.saturating_sub(1);
                if cols > 0 && rows > 0 {
                    if let Ok(master) = term.pty_master.lock() {
                        let _ = master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
                    }
                    if let Ok(mut parser) = term.parser.write() {
                        parser.screen_mut().set_size(rows, cols);
                    }
                    let _ = terminal.clear();
                }
            }
            last_terminal_size = current_size;
            last_sidebar_cols = state.sidebar_cols;
        }

        if state.focus.selected_id != last_selected_id {
            state.focus.enable_scrolling = true;

            if let Some(target_id) = &state.focus.selected_id {
                if let Some(term) = &mut active_terminal {
                    let target_id_clone = target_id.clone();
                    term.target_id = target_id.clone();
                    
                    if let Some(pid) = term.pty_pid {
                        if let Some(prev) = current_switch_task.take() {
                            prev.abort();
                        }

                        current_switch_task = Some(tokio::spawn(async move {
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
                                    cmd.args(["switch-client", "-c", &tty, "-t", &target_id_clone]);
                                    let _ = cmd.output().await;
                                }
                            }
                        }));
                    }
                } else {
                    let cols = current_size.width.saturating_sub(state.sidebar_cols);
                    let rows = current_size.height.saturating_sub(1);

                    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                    let parser = Arc::new(RwLock::new(vt100::Parser::new(rows, cols, 0)));
                    
                    let pty_system = native_pty_system();
                    let pty_rows = if rows > 0 { rows } else { 24 };
                    let pty_cols = if cols > 0 { cols } else { 80 };

                    if let Ok(pair) = pty_system.openpty(PtySize { rows: pty_rows, cols: pty_cols, pixel_width: 0, pixel_height: 0 }) {
                        let mut cmd = CommandBuilder::new("tmux");
                        // Unset TMUX to prevent nesting issues in the preview
                        cmd.env("TMUX", "");
                        cmd.env("TMUX_PANE", "");
                        let target_id_clone = target_id.clone();
                        cmd.args(["attach-session", "-t", &target_id_clone]);
                        
                        if let Ok(mut child) = pair.slave.spawn_command(cmd) {
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

                            let handle = tokio::task::spawn_blocking(move || {
                                let _ = child.wait();
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
            ui::render(f, state, &active_terminal, &last_error);
        })?;

        if event::poll(Duration::from_millis(50))? {
            let ev = event::read()?;

            // Mouse handling: separator drag takes priority, then preview passthrough
            if let Event::Mouse(mouse) = ev {
                let sep_col = state.sidebar_cols.saturating_sub(1);
                let near_separator = (mouse.column as i32 - sep_col as i32).unsigned_abs() <= 1;

                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) if near_separator => {
                        dragging_separator = true;
                        continue;
                    }
                    MouseEventKind::Drag(MouseButton::Left) if dragging_separator => {
                        let min_cols = 8u16;
                        let max_cols = current_size.width.saturating_sub(20);
                        state.sidebar_cols = (mouse.column + 1).max(min_cols).min(max_cols);
                        state.save_to_disk();
                        continue;
                    }
                    MouseEventKind::Up(MouseButton::Left) if dragging_separator => {
                        dragging_separator = false;
                        continue;
                    }
                    _ => {}
                }

                // Sidebar: click to select, scroll to navigate (works in both Tree and Preview panels)
                if mouse.column < state.sidebar_cols {
                    const TREE_Y_START: u16 = 1; // block title row
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            if mouse.row >= TREE_Y_START && mouse.row < current_size.height.saturating_sub(1) {
                                let relative_row = (mouse.row - TREE_Y_START) as usize;
                                let logical_idx = relative_row + state.focus.scroll_offset;
                                let visible = state.get_dynamic_visible_items();
                                if let Some(id) = visible.get(logical_idx) {
                                    state.focus.selected_id = Some(id.clone());
                                    state.save_to_disk();
                                }
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            state.move_selection_up();
                            state.save_to_disk();
                        }
                        MouseEventKind::ScrollDown => {
                            state.move_selection_down();
                            state.save_to_disk();
                        }
                        _ => {}
                    }
                }

                // Forward mouse events to the embedded terminal whenever the cursor is in the preview area
                if let Some(term) = &active_terminal {
                    if mouse.column >= state.sidebar_cols {
                        let x_offset = state.sidebar_cols;
                        let y_offset = 0u16;
                        if let Some(bytes) = encode_mouse(&mouse, x_offset, y_offset) {
                            let _ = term.pty_writer.send(bytes);
                        }
                    }
                }

                continue;
            }

            if let Event::Key(key) = ev {
                // Rename mode intercepts all input
                if let crate::state::InputMode::Renaming { .. } = &state.input_mode {
                    match key.code {
                        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if let crate::state::InputMode::Renaming { input, .. } = &mut state.input_mode {
                                input.push(c);
                            }
                        }
                        KeyCode::Backspace => {
                            if let crate::state::InputMode::Renaming { input, .. } = &mut state.input_mode {
                                input.pop();
                            }
                        }
                        KeyCode::Enter => {
                            if let crate::state::InputMode::Renaming { session_id, input } = state.input_mode.clone() {
                                let trimmed = input.trim().to_string();
                                if !trimmed.is_empty() {
                                    let _ = tokio::process::Command::new("tmux")
                                        .args(["rename-session", "-t", &session_id, &trimmed])
                                        .output()
                                        .await;
                                }
                            }
                            state.input_mode = crate::state::InputMode::TuiNormal;
                        }
                        KeyCode::Esc => {
                            state.input_mode = crate::state::InputMode::TuiNormal;
                        }
                        _ => {}
                    }
                    continue;
                }

                // New session mode intercepts all input
                if let crate::state::InputMode::NewSession { .. } = &state.input_mode {
                    match key.code {
                        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if let crate::state::InputMode::NewSession { input } = &mut state.input_mode {
                                input.push(c);
                            }
                        }
                        KeyCode::Backspace => {
                            if let crate::state::InputMode::NewSession { input } = &mut state.input_mode {
                                input.pop();
                            }
                        }
                        KeyCode::Enter => {
                            if let crate::state::InputMode::NewSession { input } = state.input_mode.clone() {
                                let trimmed = input.trim().to_string();
                                if !trimmed.is_empty() {
                                    let _ = tokio::process::Command::new("tmux")
                                        .args(["new-session", "-d", "-s", &trimmed])
                                        .output()
                                        .await;
                                }
                            }
                            state.input_mode = crate::state::InputMode::TuiNormal;
                        }
                        KeyCode::Esc => {
                            state.input_mode = crate::state::InputMode::TuiNormal;
                        }
                        _ => {}
                    }
                    continue;
                }

                if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL)
                    && state.focus.panel == Panel::Tree
                {
                    state.focus.panel = Panel::Preview;
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
                            state.save_to_disk();
                            disable_raw_mode()?;
                            execute!(
                                io::stdout(),
                                PopKeyboardEnhancementFlags,
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;
                            std::process::exit(0);
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            state.save_to_disk();
                            disable_raw_mode()?;
                            execute!(
                                io::stdout(),
                                PopKeyboardEnhancementFlags,
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;
                            std::process::exit(0);
                        }
                        KeyCode::Char('?') => {
                            state.show_help = true;
                        }
                        KeyCode::Enter => {
                            state.focus.panel = Panel::Preview;
                        }
                        KeyCode::Char('a') => {
                            if let Some(sel) = &state.focus.selected_id {
                                state.last_target_id = Some(sel.clone());
                                state.save_to_disk();
                                action_attach = Some(sel.clone());
                                break;
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            state.move_selection_up();
                            state.save_to_disk();
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            state.move_selection_down();
                            state.save_to_disk();
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            state.switch_nav_left();
                            state.save_to_disk();
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            state.switch_nav_right();
                            state.save_to_disk();
                        }
                        KeyCode::Char(' ') => {
                            state.toggle_expansion();
                            state.save_to_disk();
                        }
                        KeyCode::Char('n') => {
                            state.input_mode = crate::state::InputMode::NewSession {
                                input: String::new(),
                            };
                        }
                        KeyCode::Char('r') => {
                            let session_id = state.focus.selected_id.as_ref().and_then(|sel| {
                                if state.sessions.contains_key(sel) {
                                    Some(sel.clone())
                                } else if let Some(w) = state.windows.get(sel) {
                                    Some(w.session_id.clone())
                                } else if let Some(p) = state.panes.get(sel) {
                                    state.windows.get(&p.window_id).map(|w| w.session_id.clone())
                                } else {
                                    None
                                }
                            });
                            if let Some(sid) = session_id {
                                let current_name = state.sessions.get(&sid)
                                    .map(|s| s.name.clone())
                                    .unwrap_or_default();
                                state.input_mode = crate::state::InputMode::Renaming {
                                    session_id: sid,
                                    input: current_name,
                                };
                            }
                        }
                        _ => {}
                    }
                } else if state.focus.panel == Panel::Preview {
                    if prefix_pending {
                        prefix_pending = false;
                        if key.code == KeyCode::Char('d') && !key.modifiers.contains(KeyModifiers::CONTROL) {
                            state.focus.panel = Panel::Tree;
                        } else if let Some(term) = &active_terminal {
                            // Forward the swallowed Ctrl+B and then this key
                            let mut bytes = vec![0x02u8];
                            encode_key(&key, &mut bytes);
                            if !bytes.is_empty() {
                                let _ = term.pty_writer.send(bytes);
                            }
                        }
                    } else if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        prefix_pending = true;
                    } else if let Some(term) = &active_terminal {
                        let mut bytes = Vec::new();
                        encode_key(&key, &mut bytes);
                        if !bytes.is_empty() {
                            let _ = term.pty_writer.send(bytes);
                        }
                    }
                }
            }
        }
    }

    if let Some(term) = active_terminal.take() {
        if let Some(pid) = term.pty_pid {
            let _ = tokio::process::Command::new("kill")
                .arg(pid.to_string())
                .output()
                .await;
        }
    }

    if let Some(task) = pty_task.take() {
        task.abort();
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        PopKeyboardEnhancementFlags,
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Some(target) = action_attach {
        let mut cmd = tokio::process::Command::new("tmux");
        if std::env::var("TMUX").is_ok() {
            cmd.args(["switch-client", "-t", &target]);
        } else {
            cmd.env_remove("TMUX")
               .env_remove("TMUX_PANE")
               .args(["attach-session", "-t", &target]);
        }
        let mut child = cmd.spawn()?;
        let _ = child.wait().await;
    }

    Ok(())
}
