use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use indexmap::IndexMap;
use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};
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
    pub last_target_id: Option<String>,
    pub sidebar_cols: u16,
    /// Colors adopted from the user's tmux status bar at startup. Not persisted;
    /// re-read from the live tmux server each launch.
    pub theme: ttree_core::theme::Theme,
    /// Clients as of the last sync, in `pid session pane tty` form. Kept here
    /// so the preview loop can read them without spending another tmux process
    /// on every tick.
    pub clients: Vec<String>,
    /// The session ttree itself is running in, when launched from inside tmux.
    /// We refuse to mirror it (see [`AppState::mirror_suppressed`]).
    pub own_session: Option<SessionId>,
    /// Set when the selection lands in our own session, so the preview can say
    /// why it is empty instead of just going blank.
    pub mirror_suppressed: bool,
    /// Live fuzzy filter over the tree rows. `None` is no filter at all;
    /// `Some("")` is filter mode opened with nothing typed yet, which still
    /// shows everything. Deliberately not persisted: a filter is a transient
    /// way to find one session, and reopening ttree to a tree that hides most
    /// of its rows would just look broken.
    pub filter: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PersistentState {
    pub focus: Focus,
    pub expanded_ids: Vec<String>,
    #[serde(default)]
    pub sidebar_cols: u16,
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
            last_target_id: None,
            sidebar_cols: 0,
            theme: ttree_core::theme::Theme::default(),
            clients: Vec::new(),
            own_session: None,
            mirror_suppressed: false,
            filter: None,
        }
    }
}

/// One flattened tree row: the id it is selected by, the text the sidebar
/// prints for it, and the row that owns it. The parent link exists only for
/// filtering: a pane that survives has to drag its headers along with it, or
/// the tree stops saying which session the pane belongs to.
struct Row {
    id: String,
    label: String,
    parent: Option<usize>,
}

impl AppState {
    pub fn save_to_disk(&self) {
        if let Some(config_dir) = ttree_core::config::config_dir() {
            let _ = std::fs::create_dir_all(&config_dir);
            let state_path = config_dir.join("state.toml");

            let mut expanded_ids: Vec<String> = self
                .sessions
                .iter()
                .filter(|(_, s)| s.expanded)
                .map(|(id, _)| id.clone())
                .collect();
            expanded_ids
                .extend(self.windows.iter().filter(|(_, w)| w.expanded).map(|(id, _)| id.clone()));

            let persistent = PersistentState {
                focus: self.focus.clone(),
                expanded_ids,
                sidebar_cols: self.sidebar_cols,
            };

            if let Ok(toml) = toml::to_string(&persistent) {
                let _ = std::fs::write(state_path, toml);
            }
        }
    }

    pub fn load_from_disk() -> Self {
        let mut state = Self::default();
        if let Some(config_dir) = ttree_core::config::config_dir() {
            let state_path = config_dir.join("state.toml");
            if let Ok(content) = std::fs::read_to_string(state_path) {
                if let Ok(persistent) = toml::from_str::<PersistentState>(&content) {
                    state.focus = persistent.focus;
                    state.expanded_ids = persistent.expanded_ids.into_iter().collect();
                    state.sidebar_cols = persistent.sidebar_cols;
                }
            }
        }
        state
    }

    /// Flatten the tree into rows, parents before their children, in the exact
    /// order and with the exact labels `ui::tree` prints. The labels have to
    /// agree with what is on screen: the filter matches them, and a search that
    /// hit text the user cannot see would look like a bug.
    fn flatten_rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = Vec::new();
        for session in self.sessions.values() {
            let num_windows = session.windows.len();
            let total_panes: usize = session
                .windows
                .iter()
                .filter_map(|wid| self.windows.get(wid))
                .map(|w| w.panes.len())
                .sum();

            if num_windows <= 1 && total_panes <= 1 {
                // Single leaf: use pane ID (or session ID if no panes yet).
                // The row is a pane but it is labelled with the session name,
                // because that is the only name worth showing at this size.
                let leaf_id = session
                    .windows
                    .first()
                    .and_then(|wid| self.windows.get(wid))
                    .and_then(|w| w.panes.first())
                    .cloned()
                    .unwrap_or(session.id.clone());
                rows.push(Row { id: leaf_id, label: session.name.clone(), parent: None });
            } else if num_windows == 1 {
                // Session header + panes directly (window level skipped)
                let sidx = rows.len();
                rows.push(Row {
                    id: session.id.clone(),
                    label: session.name.clone(),
                    parent: None,
                });
                if session.expanded {
                    if let Some(window) =
                        session.windows.first().and_then(|wid| self.windows.get(wid))
                    {
                        for pid in &window.panes {
                            rows.push(Row {
                                id: pid.clone(),
                                label: self.pane_label(pid),
                                parent: Some(sidx),
                            });
                        }
                    }
                }
            } else {
                // Full hierarchy
                let sidx = rows.len();
                rows.push(Row {
                    id: session.id.clone(),
                    label: session.name.clone(),
                    parent: None,
                });
                if session.expanded {
                    for wid in &session.windows {
                        if let Some(window) = self.windows.get(wid) {
                            if window.panes.len() <= 1 {
                                // Window leaf: use pane ID, and the pane's name
                                // when there is a pane to take one from.
                                let leaf_id = window.panes.first().cloned().unwrap_or(wid.clone());
                                let label = window
                                    .panes
                                    .first()
                                    .and_then(|pid| self.panes.get(pid))
                                    .map(|p| p.display_name().to_string())
                                    .unwrap_or_else(|| window.name.clone());
                                rows.push(Row { id: leaf_id, label, parent: Some(sidx) });
                            } else {
                                // Window header
                                let widx = rows.len();
                                rows.push(Row {
                                    id: wid.clone(),
                                    label: window.name.clone(),
                                    parent: Some(sidx),
                                });
                                if window.expanded {
                                    for pid in &window.panes {
                                        rows.push(Row {
                                            id: pid.clone(),
                                            label: self.pane_label(pid),
                                            parent: Some(widx),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        rows
    }

    /// The label for a pane row, empty when the pane has gone away between
    /// syncs (the row still exists so ids stay stable for one more frame).
    fn pane_label(&self, pane_id: &str) -> String {
        self.panes.get(pane_id).map(|p| p.display_name().to_string()).unwrap_or_default()
    }

    /// True when the filter is actually narrowing the tree. Filter mode with an
    /// empty query is not "active": nothing is hidden, so nothing should be
    /// dimmed, annotated or explained.
    pub fn filter_active(&self) -> bool {
        self.filter.as_ref().is_some_and(|q| !q.is_empty())
    }

    pub fn get_dynamic_visible_items(&self) -> Vec<String> {
        let rows = self.flatten_rows();

        let query = match &self.filter {
            Some(q) if !q.is_empty() => q,
            // No filter, or filter mode with nothing typed yet: every row stands.
            _ => return rows.into_iter().map(|r| r.id).collect(),
        };

        let matcher = SkimMatcherV2::default();
        let mut keep = vec![false; rows.len()];
        for (i, row) in rows.iter().enumerate() {
            if matcher.fuzzy_match(&row.label, query).is_some() {
                keep[i] = true;
                // A matching pane means nothing on its own, so light up the
                // headers above it. Parents always precede their children, so
                // walking the links up can only revisit rows already decided.
                let mut parent = row.parent;
                while let Some(p) = parent {
                    keep[p] = true;
                    parent = rows[p].parent;
                }
            }
        }

        rows.into_iter().zip(keep).filter(|(_, k)| *k).map(|(row, _)| row.id).collect()
    }

    /// Put the selection back on a row that exists. Filtering can hide whatever
    /// was selected, and every action downstream (preview, attach, rename) reads
    /// the selection, so it must never point at a row nobody can see.
    /// Called from the key handling, which decides when the filter changed.
    pub fn clamp_selection_to_visible(&mut self) {
        let visible = self.get_dynamic_visible_items();
        let still_there =
            self.focus.selected_id.as_ref().is_some_and(|sel| visible.iter().any(|id| id == sel));
        if !still_there {
            self.focus.selected_id = visible.first().cloned();
        }
    }

    pub fn move_selection_up(&mut self) {
        let list = self.get_dynamic_visible_items();
        if list.is_empty() {
            return;
        }
        if let Some(sel) = &self.focus.selected_id {
            if let Some(pos) = list.iter().position(|x| x == sel) {
                let next = if pos > 0 { pos - 1 } else { list.len() - 1 };
                self.focus.selected_id = Some(list[next].clone());
                return;
            }
        }
        self.focus.selected_id = Some(list.last().unwrap().clone());
    }

    pub fn move_selection_down(&mut self) {
        let list = self.get_dynamic_visible_items();
        if list.is_empty() {
            return;
        }
        if let Some(sel) = &self.focus.selected_id {
            if let Some(pos) = list.iter().position(|x| x == sel) {
                let next = if pos + 1 < list.len() { pos + 1 } else { 0 };
                self.focus.selected_id = Some(list[next].clone());
                return;
            }
        }
        self.focus.selected_id = Some(list[0].clone());
    }

    // LEFT: collapse the selected node, or select parent if already collapsed / is a pane
    pub fn switch_nav_left(&mut self) {
        let sel = match self.focus.selected_id.clone() {
            Some(s) => s,
            None => return,
        };

        if let Some(session) = self.sessions.get_mut(&sel) {
            if session.expanded {
                session.expanded = false;
            }
            return;
        }

        if let Some(window) = self.windows.get_mut(&sel) {
            if window.expanded {
                window.expanded = false;
            } else {
                let sid = window.session_id.clone();
                self.focus.selected_id = Some(sid);
            }
            return;
        }

        if let Some(pane) = self.panes.get(&sel).cloned() {
            let wid = pane.window_id.clone();
            if let Some(window) = self.windows.get(&wid) {
                let sid = window.session_id.clone();
                let num_windows = self.sessions.get(&sid).map(|s| s.windows.len()).unwrap_or(0);
                if window.panes.len() > 1 {
                    // Parent is a window header
                    self.focus.selected_id = Some(wid);
                    if let Some(w) = self.windows.get_mut(&pane.window_id) {
                        w.expanded = false;
                    }
                } else if num_windows > 1 {
                    // Parent is a session header (window was a leaf)
                    self.focus.selected_id = Some(sid);
                } else {
                    // Single-window session with multiple panes — parent is session header
                    self.focus.selected_id = Some(sid);
                    if let Some(s) = self.sessions.get_mut(&window.session_id.clone()) {
                        s.expanded = false;
                    }
                }
            }
        }
    }

    // RIGHT: expand the selected node, or descend to first child if already expanded
    pub fn switch_nav_right(&mut self) {
        let sel = match self.focus.selected_id.clone() {
            Some(s) => s,
            None => return,
        };

        if self.sessions.contains_key(&sel) {
            let (can_expand, already_expanded) = {
                let s = self.sessions.get(&sel).unwrap();
                let num_windows = s.windows.len();
                let total_panes: usize = s
                    .windows
                    .iter()
                    .filter_map(|wid| self.windows.get(wid))
                    .map(|w| w.panes.len())
                    .sum();
                (num_windows > 1 || total_panes > 1, s.expanded)
            };
            if !can_expand {
                return;
            }
            if !already_expanded {
                self.sessions.get_mut(&sel).unwrap().expanded = true;
                return;
            }
            // Already expanded — move to first child
            let list = self.get_dynamic_visible_items();
            if let Some(pos) = list.iter().position(|x| x == &sel) {
                if pos + 1 < list.len() {
                    self.focus.selected_id = Some(list[pos + 1].clone());
                }
            }
            return;
        }

        if self.windows.contains_key(&sel) {
            let (can_expand, already_expanded) = {
                let w = self.windows.get(&sel).unwrap();
                (w.panes.len() > 1, w.expanded)
            };
            if !can_expand {
                return;
            }
            if !already_expanded {
                self.windows.get_mut(&sel).unwrap().expanded = true;
                return;
            }
            // Already expanded — move to first pane
            if let Some(pid) = self.windows.get(&sel).and_then(|w| w.panes.first()).cloned() {
                self.focus.selected_id = Some(pid);
            }
        }
        // Panes are leaves — nothing to expand
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
    pub last_attached: i64,
    pub attached: bool,
}

#[derive(Debug, Clone)]
pub struct Window {
    #[allow(dead_code)]
    pub id: WindowId,
    pub session_id: SessionId,
    pub name: String,
    pub panes: Vec<PaneId>,
    pub active: bool,
    /// Unix time of the window's last activity, straight from tmux. Compared
    /// against the clock rather than tmux's own activity flag, which only gets
    /// set when the user has `monitor-activity` on.
    pub last_activity: i64,
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
        f.debug_struct("EmbeddedTerminal").field("target_id", &self.target_id).finish()
    }
}

#[derive(Debug, Clone)]
pub struct Pane {
    pub id: PaneId,
    pub window_id: WindowId,
    pub title: String,
    pub current_command: String,
    pub active: bool,
    #[allow(dead_code)]
    pub region: Option<Rect>,
}

impl Pane {
    /// What the tree prints for a pane: the running command when tmux reports
    /// one, else the pane title. Lives here rather than in the widget so the
    /// filter matches the same text the sidebar draws.
    pub fn display_name(&self) -> &str {
        if !self.current_command.is_empty() {
            &self.current_command
        } else {
            &self.title
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputMode {
    TuiNormal,
    /// Renaming whatever is selected. The target is a session or a window id,
    /// and which one decides the tmux command that eventually runs.
    Renaming {
        target_id: String,
        input: String,
    },
    /// A destructive action holding for a y/n. Kills cannot be undone, so they
    /// never ride on a single keystroke.
    Confirming {
        target_id: String,
        prompt: String,
    },
    NewSession {
        input: String,
    },
    /// Typing a fuzzy filter into the command bar. The query also lives in
    /// [`AppState::filter`], which is what the tree reads: the mode is only
    /// about where keystrokes go, so leaving it keeps the filter applied.
    Filtering {
        input: String,
    },
    #[allow(dead_code)]
    PtyPassthrough {
        pane_id: PaneId,
    },
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
        self.get_dynamic_visible_items()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, windows: &[&str], expanded: bool) -> Session {
        Session {
            id: id.into(),
            name: id.trim_start_matches('$').into(),
            windows: windows.iter().map(|w| w.to_string()).collect(),
            expanded,
            last_attached: 0,
            attached: false,
        }
    }

    fn window(id: &str, session_id: &str, panes: &[&str], expanded: bool) -> Window {
        Window {
            id: id.into(),
            session_id: session_id.into(),
            name: "win".into(),
            panes: panes.iter().map(|p| p.to_string()).collect(),
            active: true,
            last_activity: 0,
            width: 80,
            height: 24,
            expanded,
        }
    }

    fn pane(id: &str, window_id: &str) -> Pane {
        Pane {
            id: id.into(),
            window_id: window_id.into(),
            title: "sh".into(),
            current_command: "sh".into(),
            active: true,
            region: None,
        }
    }

    /// Window id, expanded, and its pane ids.
    type WindowSpec<'a> = (&'a str, bool, &'a [&'a str]);
    /// Session id, expanded, and its windows.
    type SessionSpec<'a> = (&'a str, bool, &'a [WindowSpec<'a>]);

    /// Build a state from (session, windows, panes-per-window) descriptions.
    fn state_with(spec: &[SessionSpec]) -> AppState {
        let mut st = AppState::default();
        for (sid, sexp, windows) in spec {
            let wids: Vec<&str> = windows.iter().map(|(wid, _, _)| *wid).collect();
            st.sessions.insert(sid.to_string(), session(sid, &wids, *sexp));
            for (wid, wexp, panes) in *windows {
                st.windows.insert(wid.to_string(), window(wid, sid, panes, *wexp));
                for pid in *panes {
                    st.panes.insert(pid.to_string(), pane(pid, wid));
                }
            }
        }
        st
    }

    #[test]
    fn a_lone_pane_collapses_to_just_that_pane() {
        // One window, one pane: no hierarchy is worth showing, so the row *is*
        // the pane and the session header is skipped entirely.
        let st = state_with(&[("$0", true, &[("@0", true, &["%0"])])]);
        assert_eq!(st.get_dynamic_visible_items(), vec!["%0"]);
    }

    #[test]
    fn a_single_window_session_skips_the_window_row() {
        let st = state_with(&[("$0", true, &[("@0", true, &["%0", "%1"])])]);
        assert_eq!(st.get_dynamic_visible_items(), vec!["$0", "%0", "%1"]);
    }

    #[test]
    fn collapsing_a_session_hides_its_panes() {
        let st = state_with(&[("$0", false, &[("@0", true, &["%0", "%1"])])]);
        assert_eq!(st.get_dynamic_visible_items(), vec!["$0"]);
    }

    #[test]
    fn multi_window_sessions_show_the_full_hierarchy() {
        let st = state_with(&[("$0", true, &[("@0", true, &["%0", "%1"]), ("@1", true, &["%2"])])]);
        // @0 has two panes so it gets a header row; @1 has one, so its row is
        // the pane itself.
        assert_eq!(st.get_dynamic_visible_items(), vec!["$0", "@0", "%0", "%1", "%2"]);
    }

    #[test]
    fn collapsing_a_window_hides_only_its_own_panes() {
        let st =
            state_with(&[("$0", true, &[("@0", false, &["%0", "%1"]), ("@1", true, &["%2"])])]);
        assert_eq!(st.get_dynamic_visible_items(), vec!["$0", "@0", "%2"]);
    }

    #[test]
    fn selection_wraps_at_both_ends() {
        let mut st = state_with(&[("$0", true, &[("@0", true, &["%0", "%1"])])]);
        st.focus.selected_id = Some("%1".into()); // last row

        st.move_selection_down();
        assert_eq!(st.focus.selected_id.as_deref(), Some("$0"), "past the end wraps to the top");

        st.move_selection_up();
        assert_eq!(st.focus.selected_id.as_deref(), Some("%1"), "before the top wraps to the end");
    }

    #[test]
    fn a_selection_that_left_the_list_falls_back_to_an_end() {
        // e.g. the selected pane closed between syncs.
        let mut st = state_with(&[("$0", true, &[("@0", true, &["%0", "%1"])])]);
        st.focus.selected_id = Some("%99".into());
        st.move_selection_down();
        assert_eq!(st.focus.selected_id.as_deref(), Some("$0"));

        st.focus.selected_id = Some("%99".into());
        st.move_selection_up();
        assert_eq!(st.focus.selected_id.as_deref(), Some("%1"));
    }

    #[test]
    fn selection_moves_are_safe_with_no_sessions() {
        let mut st = AppState::default();
        st.move_selection_down();
        st.move_selection_up();
        assert_eq!(st.focus.selected_id, None);
    }

    /// Two sessions: `$alpha` needs a header (two panes), `$beta` collapses to
    /// one leaf row. Panes are named "sh" unless a test renames them.
    fn filterable_state() -> AppState {
        state_with(&[
            ("$alpha", true, &[("@0", true, &["%0", "%1"])]),
            ("$beta", true, &[("@1", true, &["%2"])]),
        ])
    }

    #[test]
    fn no_filter_shows_every_row() {
        let mut st = filterable_state();
        assert_eq!(st.filter, None);
        assert!(!st.filter_active());
        assert_eq!(st.get_dynamic_visible_items(), vec!["$alpha", "%0", "%1", "%2"]);

        // Filter mode open with nothing typed hides nothing: the tree should not
        // flicker away the moment the key is pressed.
        st.filter = Some(String::new());
        assert!(!st.filter_active());
        assert_eq!(st.get_dynamic_visible_items(), vec!["$alpha", "%0", "%1", "%2"]);
    }

    #[test]
    fn a_matching_pane_keeps_its_session_header() {
        let mut st = filterable_state();
        st.panes.get_mut("%1").unwrap().current_command = "vim".into();
        st.filter = Some("vim".into());

        assert!(st.filter_active());
        // $alpha survives only as %1's ancestor; %0 ("sh") and %2 ("beta") go.
        assert_eq!(st.get_dynamic_visible_items(), vec!["$alpha", "%1"]);
    }

    #[test]
    fn a_matching_pane_keeps_its_window_header_too() {
        // Two windows, so the session shows the full hierarchy and the match
        // sits two levels deep.
        let mut st =
            state_with(&[("$s", true, &[("@a", true, &["%0", "%1"]), ("@b", true, &["%2"])])]);
        st.panes.get_mut("%1").unwrap().current_command = "htop".into();
        st.filter = Some("htop".into());

        assert_eq!(st.get_dynamic_visible_items(), vec!["$s", "@a", "%1"]);
    }

    #[test]
    fn a_filter_matching_nothing_empties_the_tree() {
        let mut st = filterable_state();
        st.filter = Some("zzzz".into());
        assert!(st.get_dynamic_visible_items().is_empty());
    }

    #[test]
    fn filtering_out_the_selection_moves_it_to_the_first_visible_row() {
        let mut st = filterable_state();
        st.panes.get_mut("%1").unwrap().current_command = "vim".into();
        st.focus.selected_id = Some("%2".into()); // in $beta, about to be hidden

        st.filter = Some("vim".into());
        st.clamp_selection_to_visible();
        assert_eq!(st.focus.selected_id.as_deref(), Some("$alpha"));

        // A selection that survives the filter is left where it is.
        st.focus.selected_id = Some("%1".into());
        st.clamp_selection_to_visible();
        assert_eq!(st.focus.selected_id.as_deref(), Some("%1"));
    }
}
// probe
