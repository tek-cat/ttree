use indexmap::IndexMap;
use ratatui::layout::Rect;
use serde::{Serialize, Deserialize};
use std::collections::HashSet;

pub type SessionId = String;
pub type WindowId = String;
pub type PaneId = String;

#[derive(Debug, Clone)]
pub struct AppState {
    pub sessions: IndexMap<SessionId, Session>,
    pub windows: IndexMap<WindowId, Window>,
    pub panes: IndexMap<PaneId, Pane>,
    pub focus: Focus,
    pub input_mode: InputMode,
    pub show_help: bool,
    pub expanded_ids: HashSet<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PersistentState {
    pub focus: Focus,
    pub expanded_ids: Vec<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            sessions: IndexMap::new(),
            windows: IndexMap::new(),
            panes: IndexMap::new(),
            focus: Focus::default(),
            input_mode: InputMode::TuiNormal,
            show_help: false,
            expanded_ids: HashSet::new(),
        }
    }
}

impl AppState {
    pub fn save_to_disk(&self) {
        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "tek", "ttree") {
            let config_dir = proj_dirs.config_dir();
            let _ = std::fs::create_dir_all(config_dir);
            let state_path = config_dir.join("state.toml");
            
            let mut expanded_ids: Vec<String> = self.sessions.iter()
                .filter(|(_, s)| s.expanded)
                .map(|(id, _)| id.clone())
                .collect();
            expanded_ids.extend(self.windows.iter()
                .filter(|(_, w)| w.expanded)
                .map(|(id, _)| id.clone()));
            
            let persistent = PersistentState {
                focus: self.focus.clone(),
                expanded_ids,
            };
            
            if let Ok(toml) = toml::to_string(&persistent) {
                let _ = std::fs::write(state_path, toml);
            }
        }
    }

    pub fn load_from_disk() -> Self {
        let mut state = Self::default();
        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "tek", "ttree") {
            let state_path = proj_dirs.config_dir().join("state.toml");
            if let Ok(content) = std::fs::read_to_string(state_path) {
                if let Ok(persistent) = toml::from_str::<PersistentState>(&content) {
                    state.focus = persistent.focus;
                    state.expanded_ids = persistent.expanded_ids.into_iter().collect();
                }
            }
        }
        state
    }

    pub fn get_flat_list(&self, mode: &NavMode) -> Vec<String> {
        let mut list = Vec::new();
        for session in self.sessions.values() {
            if mode == &NavMode::Session {
                list.push(session.id.clone());
            } else {
                if !session.expanded {
                    continue;
                }
                for window_id in &session.windows {
                    if mode == &NavMode::Window {
                        list.push(window_id.clone());
                    } else if mode == &NavMode::Pane {
                        if let Some(window) = self.windows.get(window_id) {
                            if !window.expanded {
                                continue;
                            }
                            for pane_id in &window.panes {
                                list.push(pane_id.clone());
                            }
                        }
                    }
                }
            }
        }
        list
    }

    pub fn move_selection_up(&mut self) {
        let list = self.get_flat_list(&self.focus.nav_mode);
        if list.is_empty() {
            return;
        }

        if let Some(sel) = &self.focus.selected_id {
            if let Some(pos) = list.iter().position(|x| x == sel) {
                if pos > 0 {
                    self.focus.selected_id = Some(list[pos - 1].clone());
                }
                return;
            }
        }

        // If nothing selected or selection not in list, pick the last one for "up"
        self.focus.selected_id = Some(list.last().unwrap().clone());
    }

    pub fn move_selection_down(&mut self) {
        let list = self.get_flat_list(&self.focus.nav_mode);
        if list.is_empty() {
            return;
        }

        if let Some(sel) = &self.focus.selected_id {
            if let Some(pos) = list.iter().position(|x| x == sel) {
                if pos + 1 < list.len() {
                    self.focus.selected_id = Some(list[pos + 1].clone());
                }
                return;
            }
        }

        // If nothing selected or selection not in list, pick the first one
        self.focus.selected_id = Some(list[0].clone());
    }

    pub fn switch_nav_left(&mut self) {
        match self.focus.nav_mode {
            NavMode::Pane => {
                self.focus.nav_mode = NavMode::Window;
                if let Some(sel) = &self.focus.selected_id {
                    if let Some(pane) = self.panes.get(sel) {
                        self.focus.selected_id = Some(pane.window_id.clone());
                    }
                }
            }
            NavMode::Window => {
                self.focus.nav_mode = NavMode::Session;
                if let Some(sel) = &self.focus.selected_id {
                    if let Some(window) = self.windows.get(sel) {
                        self.focus.selected_id = Some(window.session_id.clone());
                    }
                }
            }
            NavMode::Session => {}
        }
        self.ensure_selected_expanded();
    }

    pub fn switch_nav_right(&mut self) {
        match self.focus.nav_mode {
            NavMode::Session => {
                self.focus.nav_mode = NavMode::Window;
                if let Some(sel) = &self.focus.selected_id {
                    if let Some(session) = self.sessions.get(sel) {
                        if !session.windows.is_empty() {
                            self.focus.selected_id = Some(session.windows[0].clone());
                        }
                    }
                }
            }
            NavMode::Window => {
                self.focus.nav_mode = NavMode::Pane;
                if let Some(sel) = &self.focus.selected_id {
                    if let Some(window) = self.windows.get(sel) {
                        if !window.panes.is_empty() {
                            self.focus.selected_id = Some(window.panes[0].clone());
                        }
                    }
                }
            }
            NavMode::Pane => {}
        }
        self.ensure_selected_expanded();
    }

    fn ensure_selected_expanded(&mut self) {
        if let Some(sel) = &self.focus.selected_id {
            if let Some(window) = self.windows.get(sel) {
                if let Some(session) = self.sessions.get_mut(&window.session_id) {
                    session.expanded = true;
                }
            } else if let Some(pane) = self.panes.get(sel) {
                if let Some(window) = self.windows.get_mut(&pane.window_id) {
                    window.expanded = true;
                    if let Some(session) = self.sessions.get_mut(&window.session_id) {
                        session.expanded = true;
                    }
                }
            }
        }
    }

    pub fn toggle_expansion(&mut self) {
        if let Some(sel) = &self.focus.selected_id {
            if let Some(session) = self.sessions.get_mut(sel) {
                session.expanded = !session.expanded;
            } else if let Some(window) = self.windows.get_mut(sel) {
                window.expanded = !window.expanded;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub windows: Vec<WindowId>,
    pub expanded: bool,
}

#[derive(Debug, Clone)]
pub struct Window {
    pub id: WindowId,
    pub session_id: SessionId,
    pub name: String,
    pub panes: Vec<PaneId>,
    pub active: bool,
    #[allow(dead_code)]
    pub width: u16,
    #[allow(dead_code)]
    pub height: u16,
    pub expanded: bool,
}

#[derive(Clone)]
pub struct EmbeddedTerminal {
    pub parser: std::sync::Arc<std::sync::RwLock<vt100::Parser>>,
    pub pty_writer: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    pub target_id: String,
    pub pty_pid: Option<u32>,
    #[allow(dead_code)]
    pub pty_master: std::sync::Arc<std::sync::Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
}

impl std::fmt::Debug for EmbeddedTerminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddedTerminal")
            .field("target_id", &self.target_id)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct Pane {
    pub id: PaneId,
    pub window_id: WindowId,
    pub title: String,
    pub current_command: String,
    pub active: bool,
    pub region: Option<Rect>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputMode {
    TuiNormal,
    #[allow(dead_code)]
    PtyPassthrough { pane_id: PaneId },
    #[allow(dead_code)]
    FuzzySearch,
    #[allow(dead_code)]
    Command,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Focus {
    pub panel: Panel,
    pub nav_mode: NavMode,
    pub selected_id: Option<String>,
    pub scroll_offset: usize,
    pub enable_scrolling: bool,
}


impl AppState {
    pub fn get_visible_list(&self) -> Vec<String> {
        let mut list = Vec::new();
        for session in self.sessions.values() {
            list.push(session.id.clone());
            if session.expanded {
                for window_id in &session.windows {
                    list.push(window_id.clone());
                    if let Some(window) = self.windows.get(window_id) {
                        if window.expanded {
                            for pane_id in &window.panes {
                                list.push(pane_id.clone());
                            }
                        }
                    }
                }
            }
        }
        list
    }

    pub fn update_scroll(&mut self, height: usize) {
        let visible = self.get_visible_list();
        if let Some(selected_id) = &self.focus.selected_id {
            if let Some(pos) = visible.iter().position(|id| id == selected_id) {
                if pos < self.focus.scroll_offset {
                    self.focus.scroll_offset = pos;
                } else if pos >= self.focus.scroll_offset + height {
                    self.focus.scroll_offset = pos - height + 1;
                }
            }
        }

        // Clamp scroll_offset
        if !visible.is_empty() {
            let max_offset = visible.len().saturating_sub(height);
            if self.focus.scroll_offset > max_offset {
                self.focus.scroll_offset = max_offset;
            }
        } else {
            self.focus.scroll_offset = 0;
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum NavMode {
    Session,
    #[default]
    Window,
    Pane,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Panel {
    #[default]
    Tree,
    Preview,
}
