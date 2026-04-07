use indexmap::IndexMap;
use ratatui::layout::Rect;

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
    pub cmd_counter: u64,
    pub show_help: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            sessions: IndexMap::new(),
            windows: IndexMap::new(),
            panes: IndexMap::new(),
            focus: Focus::default(),
            input_mode: InputMode::TuiNormal,
            cmd_counter: 0,
            show_help: false,
        }
    }
}

impl AppState {
    fn get_flat_list(&self) -> Vec<String> {
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

    pub fn move_selection_up(&mut self) {
        let list = self.get_flat_list();
        if list.is_empty() { return; }
        
        let mut idx = 0;
        if let Some(sel) = &self.focus.selected_id {
            idx = list.iter().position(|x| x == sel).unwrap_or(0);
        }
        
        if idx > 0 {
            self.focus.selected_id = Some(list[idx - 1].clone());
        }
    }

    pub fn move_selection_down(&mut self) {
        let list = self.get_flat_list();
        if list.is_empty() { return; }
        
        let mut idx = 0;
        if let Some(sel) = &self.focus.selected_id {
            idx = list.iter().position(|x| x == sel).unwrap_or(0);
        }
        
        if idx + 1 < list.len() {
            self.focus.selected_id = Some(list[idx + 1].clone());
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
    pub width: u16,
    pub height: u16,
    pub expanded: bool,
}

#[derive(Debug, Clone)]
pub struct PanePreview {
    pub id: PaneId,
    pub region: Rect,
    pub content: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct WindowPreview {
    pub window_id: WindowId,
    pub width: u16,
    pub height: u16,
    pub panes: Vec<PanePreview>,
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

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    TuiNormal,
    PtyPassthrough { pane_id: PaneId },
    FuzzySearch,
    Command,
}

#[derive(Debug, Clone, Default)]
pub struct Focus {
    pub panel: Panel,
    pub selected_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Panel {
    #[default]
    Tree,
    Preview,
}
