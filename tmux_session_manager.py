import sys
import json
import os
import subprocess
import libtmux
from textual.app import App, ComposeResult
from textual.widgets import DataTable, Header, Footer, Input, Label, Button
from textual.binding import Binding
from textual.theme import Theme
from textual import on, work
from textual.screen import ModalScreen
from textual.containers import Vertical, Horizontal

from rich.text import Text
from rich.markup import escape

CONFIG_FILE = os.path.expanduser("~/.tmux_session_manager_config.json")

def load_config():
    if os.path.exists(CONFIG_FILE):
        try:
            with open(CONFIG_FILE, "r") as f:
                return json.load(f)
        except:
            return {}
    return {}

def save_config(config):
    try:
        with open(CONFIG_FILE, "w") as f:
            json.dump(config, f)
    except:
        pass

def log_debug(message: str) -> None:
    try:
        with open("/tmp/tmux_sm.log", "a") as f:
            f.write(message + "\n")
    except:
        pass

ROSE_PINE = Theme(
    name="rose-pine",
    primary="#c4a7e7",    # Iris
    secondary="#31748f",  # Pine
    accent="#eb6f92",     # Love
    foreground="#e0def4", # Text
    background="#191724", # Base
    success="#9ccfd8",    # Foam
    warning="#f6c177",    # Gold
    error="#eb6f92",      # Love
    surface="#1f1d2e",    # Surface
    panel="#26233a",      # Overlay
    dark=True,
)

class RenameModal(ModalScreen[str]):
    """A modal screen to rename a session or window."""

    def __init__(self, old_name: str, type_label: str):
        super().__init__()
        self.old_name = old_name
        self.type_label = type_label

    def compose(self) -> ComposeResult:
        with Vertical(id="rename_dialog"):
            yield Label(f"Rename {self.type_label}", id="rename_title")
            yield Input(value=self.old_name, id="rename_input")
            with Horizontal():
                yield Button("Rename", variant="primary", id="rename_confirm")
                yield Button("Cancel", variant="error", id="rename_cancel")

    def on_mount(self) -> None:
        self.query_one("#rename_input", Input).focus()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "rename_confirm":
            self.dismiss(self.query_one("#rename_input", Input).value)
        else:
            self.dismiss(None)

    def on_key(self, event) -> None:
        event.stop()
        if event.key == "enter":
            self.dismiss(self.query_one("#rename_input", Input).value)
        elif event.key == "escape":
            self.dismiss(None)

class NewSessionModal(ModalScreen[str]):
    """A modal screen to create a new session."""

    def compose(self) -> ComposeResult:
        with Vertical(id="rename_dialog"):
            yield Label("New Session Name", id="rename_title")
            yield Input(placeholder="session name", id="rename_input")
            with Horizontal():
                yield Button("Create", variant="primary", id="rename_confirm")
                yield Button("Cancel", variant="error", id="rename_cancel")

    def on_mount(self) -> None:
        self.query_one("#rename_input", Input).focus()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "rename_confirm":
            self.dismiss(self.query_one("#rename_input", Input).value)
        else:
            self.dismiss(None)

    def on_key(self, event) -> None:
        event.stop()
        if event.key == "enter":
            self.dismiss(self.query_one("#rename_input", Input).value)
        elif event.key == "escape":
            self.dismiss(None)

class DeleteConfirmModal(ModalScreen[bool]):
    """A modal screen to confirm deletion."""
    
    def __init__(self, target_name: str):
        super().__init__()
        self.target_name = target_name

    def compose(self) -> ComposeResult:
        with Vertical(id="delete_dialog"):
            yield Label("Confirm Deletion", id="delete_title")
            yield Label(f"Are you sure you want to delete '{self.target_name}'?", id="delete_message")
            yield Label("Press [bold]d[/bold] again to confirm.", id="delete_hint")
            yield Label("Any other key will cancel.", id="delete_cancel_hint")
            with Horizontal():
                yield Button("Delete", variant="error", id="delete_confirm")
                yield Button("Cancel", variant="primary", id="delete_cancel")

    def on_key(self, event) -> None:
        event.stop()
        if event.key == "d":
            self.dismiss(True)
        else:
            self.dismiss(False)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "delete_confirm":
            self.dismiss(True)
        else:
            self.dismiss(False)

class TmuxSessionManagerApp(App):
    """A Textual app to manage tmux sessions in a table view."""

    TITLE = "Tmux Session Manager"
    CSS = """
    DataTable {
        height: 100%;
        border: solid $secondary;
    }
    #rename_dialog {
        padding: 1 2;
        width: 50;
        height: auto;
        border: thick $primary;
        background: $surface;
        align: center middle;
    }
    #rename_title {
        width: 100%;
        content-align: center middle;
        margin-bottom: 1;
        text-style: bold;
    }
    #rename_input {
        margin-bottom: 1;
    }
    #delete_dialog {
        padding: 1 2;
        width: 50;
        height: auto;
        border: thick $error;
        background: $surface;
        align: center middle;
    }
    #delete_title {
        width: 100%;
        content-align: center middle;
        margin-bottom: 1;
        text-style: bold;
        color: $error;
    }
    #delete_message {
        margin-bottom: 1;
        content-align: center middle;
    }
    #delete_hint {
        margin-bottom: 0;
        content-align: center middle;
        color: $warning;
    }
    #delete_cancel_hint {
        margin-bottom: 1;
        content-align: center middle;
        color: $secondary;
        opacity: 0.7;
    }
    Horizontal {
        height: auto;
        align: center middle;
    }
    Horizontal Button {
        margin: 0 1;
    }
    """
    
    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("n", "new_session", "New Session"),
        Binding("w", "rename_window", "Rename Window"),
        Binding("s", "rename_session", "Rename Session"),
        Binding("d", "delete_target", "Delete Window"),
        Binding("N", "toggle_notifications", "Toggle Notification"),
        Binding("R", "refresh_table", "Refresh"),
        Binding("j", "cursor_down", "Down", show=False),
        Binding("k", "cursor_up", "Up", show=False),
        Binding("h", "cursor_left", "Left", show=False),
        Binding("l", "cursor_right", "Right", show=False),
    ]

    def action_new_session(self) -> None:
        """Create a new tmux session."""
        def handle_new_session(name: str | None) -> None:
            if name:
                try:
                    session = self.server.new_session(session_name=name)
                    if session and session.windows:
                        # Set target for populate_table to jump to
                        self.target_window_after_refresh = session.windows[0].id
                    self.populate_table()
                except Exception as e:
                    pass

        self.push_screen(NewSessionModal(), handle_new_session)

    def action_cursor_down(self) -> None:
        table = self.query_one(DataTable)
        if table.cursor_coordinate.column == 1:  # Session column
            current_row = table.cursor_coordinate.row
            row_keys = list(table.rows.keys())
            if not row_keys:
                return
            
            current_meta = self.row_metadata.get(row_keys[current_row].value)
            if not current_meta:
                return
            
            # Find the first row of the *next* session
            next_session_id = None
            for r in range(current_row + 1, len(row_keys)):
                meta = self.row_metadata.get(row_keys[r].value)
                if meta and meta["session_id"] != current_meta["session_id"]:
                    next_session_id = meta["session_id"]
                    break
            
            if next_session_id:
                # Find the target window row for that session
                last_w_id = self.session_last_window.get(next_session_id)
                target_r = -1
                if last_w_id:
                    try:
                        target_r = table.get_row_index(last_w_id)
                    except Exception:
                        pass
                
                if target_r == -1:
                    # Default to the first row of that session (the one that has display_session_name)
                    for r in range(current_row + 1, len(row_keys)):
                        meta = self.row_metadata.get(row_keys[r].value)
                        if meta and meta["session_id"] == next_session_id:
                            target_r = r
                            break
                
                if target_r != -1:
                    table.move_cursor(row=target_r, column=1)
                    return
        else:
            table.action_cursor_down()

    def action_cursor_up(self) -> None:
        table = self.query_one(DataTable)
        if table.cursor_coordinate.column == 1:  # Session column
            current_row = table.cursor_coordinate.row
            row_keys = list(table.rows.keys())
            if not row_keys:
                return
            
            current_meta = self.row_metadata.get(row_keys[current_row].value)
            if not current_meta:
                return
            
            # Find the *first* row of the *previous* session
            prev_session_id = None
            for r in range(current_row - 1, -1, -1):
                meta = self.row_metadata.get(row_keys[r].value)
                if meta and meta["session_id"] != current_meta["session_id"]:
                    prev_session_id = meta["session_id"]
                    break
            
            if prev_session_id:
                # Find the target window row for that session
                last_w_id = self.session_last_window.get(prev_session_id)
                target_r = -1
                if last_w_id:
                    try:
                        target_r = table.get_row_index(last_w_id)
                    except Exception:
                        pass
                
                if target_r == -1:
                    # Default to the first row of that session (the one that has display_session_name)
                    # We need to find the *first* row of this session
                    for r in range(0, current_row):
                        meta = self.row_metadata.get(row_keys[r].value)
                        if meta and meta["session_id"] == prev_session_id:
                            target_r = r
                            break
                
                if target_r != -1:
                    table.move_cursor(row=target_r, column=1)
                    return
        else:
            table.action_cursor_up()

    def action_cursor_left(self) -> None:
        table = self.query_one(DataTable)
        table.move_cursor(column=1)

    def action_cursor_right(self) -> None:
        table = self.query_one(DataTable)
        table.move_cursor(column=2)

    def __init__(self):
        super().__init__()
        self.server = libtmux.Server()
        self.is_refreshing = False
        self.last_switched_target = None
        self.session_last_window = {}
        # Store metadata about rows: {row_key: metadata_dict}
        self.row_metadata = {}
        self.config = load_config()
        self.first_load = True
        self.notify_enabled_windows = set()

    def action_quit(self) -> None:
        """Save configuration and quit."""
        table = self.query_one(DataTable)
        if table.cursor_row is not None:
            try:
                row_key = table.coordinate_to_cell_key(table.cursor_coordinate).row_key
                metadata = self.row_metadata.get(row_key.value)
                if metadata:
                    self.config["last_session_name"] = metadata["session_name"]
                    self.config["last_window_name"] = metadata["window_name"]
                    save_config(self.config)
            except Exception:
                pass
        self.exit()

    def compose(self) -> ComposeResult:
        """Create child widgets for the app."""
        yield Header()
        yield DataTable(cursor_type="cell")
        yield Footer()

    def on_mount(self) -> None:
        """Populate the table when the app starts and start the refresh timer."""
        self.register_theme(ROSE_PINE)
        self.theme = "rose-pine"
        table = self.query_one(DataTable)
        table.add_column("S", width=3)
        table.add_column("Session")
        table.add_column("Window")
        self.populate_table()
        self.set_interval(1.0, self.populate_table)
        table.focus()
        # Default to Session column
        table.move_cursor(column=1)

    def action_refresh_table(self) -> None:
        """Refresh the table data."""
        self.populate_table()

    def action_rename_window(self) -> None:
        """Rename the currently selected window."""
        table = self.query_one(DataTable)
        if table.cursor_row is None:
            return
        
        row_key = table.coordinate_to_cell_key(table.cursor_coordinate).row_key
        metadata = self.row_metadata.get(row_key.value)
        if not metadata:
            return

        old_name = metadata["window_name"]

        def handle_rename(new_name: str | None) -> None:
            if new_name and new_name != old_name:
                self.perform_rename(metadata, "window", new_name)

        self.push_screen(RenameModal(old_name, "Window"), handle_rename)

    def action_rename_session(self) -> None:
        """Rename the session of the currently selected window."""
        table = self.query_one(DataTable)
        if table.cursor_row is None:
            return
        
        row_key = table.coordinate_to_cell_key(table.cursor_coordinate).row_key
        metadata = self.row_metadata.get(row_key.value)
        if not metadata:
            return

        old_name = metadata["session_name"]

        def handle_rename(new_name: str | None) -> None:
            if new_name and new_name != old_name:
                self.perform_rename(metadata, "session", new_name)

        self.push_screen(RenameModal(old_name, "Session"), handle_rename)

    def perform_rename(self, metadata: dict, type: str, new_name: str) -> None:
        """Perform the actual rename using libtmux."""
        try:
            if type == "session":
                session = self.server.sessions.get(id=metadata["session_id"])
                if session:
                    session.rename_session(new_name)
            elif type == "window":
                session = self.server.sessions.get(id=metadata["session_id"])
                if session:
                    window = session.windows.get(id=metadata["window_id"])
                    if window:
                        window.rename_window(new_name)
            
            self.populate_table()
        except Exception as e:
            pass

    def action_delete_target(self) -> None:
        """Delete the currently selected window/session."""
        table = self.query_one(DataTable)
        if table.cursor_row is None:
            return
        
        row_key = table.coordinate_to_cell_key(table.cursor_coordinate).row_key
        metadata = self.row_metadata.get(row_key.value)
        if not metadata:
            return

        def handle_delete(confirmed: bool) -> None:
            if confirmed:
                try:
                    session = self.server.sessions.get(id=metadata["session_id"])
                    if session:
                        window = session.windows.get(id=metadata["window_id"])
                        if window:
                            # Reset last switched target to force a switch after refresh
                            self.last_switched_target = None
                            window.kill()
                    
                    # Explicitly refresh immediately
                    self.populate_table()
                except Exception as e:
                    pass

        self.push_screen(DeleteConfirmModal(metadata["window_name"]), handle_delete)

    def action_toggle_notifications(self) -> None:
        """Toggle system notifications for the currently selected window."""
        table = self.query_one(DataTable)
        if table.cursor_row is None:
            return
        
        row_key = table.coordinate_to_cell_key(table.cursor_coordinate).row_key
        metadata = self.row_metadata.get(row_key.value)
        if not metadata:
            return

        w_id = metadata["window_id"]
        if w_id in self.notify_enabled_windows:
            self.notify_enabled_windows.remove(w_id)
        else:
            self.notify_enabled_windows.add(w_id)
        
        self.populate_table()

    @work(thread=True)
    def trigger_notification(self, window_name: str, session_name: str) -> None:
        """Send a system notification."""
        try:
            subprocess.run(
                ["notify-send", "Tmux Task Finished", f"Window '{window_name}' in session '{session_name}' is Ready!"],
                check=False
            )
        except:
            pass

    def get_window_status(self, raw_w_name: str, pane_title: str, cmd: str) -> tuple[str, str]:
        """Detect status from metadata. Returns (icon, color)."""
        w_name_lower = raw_w_name.lower()
        pane_title = pane_title.strip()
        combined_title = (raw_w_name + " " + pane_title)
        combined_title_lower = combined_title.lower()
        shells = ["bash", "zsh", "fish", "sh", "tmux"]
        
        # 0. Shell/Idle -> Green
        if cmd in shells or pane_title.lower() in shells:
            return "●", "green"

        # 1. Thinking/Working signals (Icons and Keywords) -> Yellow
        # These take precedence over any 'Ready' icons that might still be in the title.
        # \u2726 = ✦ (Working), \u2802 = ⠂ (Claude Working)
        working_icons = ["\u2726", "\u2802", "✦", "✸", "★", "✨", "❗", "✋", "⚠", "⏳", "⌛", "🔄", "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
        working_keywords = ["thinking", "working", "processing", "running", "thought", "analyzing", "generating", "executing", "searching"]
        
        if any(icon in combined_title for icon in working_icons) or \
           any(kw in combined_title_lower for kw in working_keywords):
            return "●", "yellow"

        if "waiting" in combined_title_lower and "input" not in combined_title_lower:
            return "●", "yellow"

        # 2. Explicit Green/Ready signals -> Green
        # \u25c7 = ◇ (Ready), \u2733 = ✳ (Claude Task Done)
        ready_icons = ["\u25c7", "◇", "✓", "✔", "✅", "✳", "\u2733"]
        if any(icon in combined_title for icon in ready_icons) or \
           ("ready" in combined_title_lower and "not ready" not in combined_title_lower):
            return "●", "green"

        # 3. Heuristic for LLM tools
        llm_tools = ["gemini", "claude", "opencode", "gpt", "anthropic", "openai", "ollama", "llama", "ai-", "assistant",
                     "g1", "g2", "g3", "g4", "g5", "g6", "g7", "g8", "g9",
                     "c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8", "c9"]
        
        is_llm = any(tool in cmd for tool in llm_tools) or \
                 any(tool == w_name_lower or f"{tool}:" in w_name_lower or f" {tool}" in w_name_lower for tool in llm_tools)
        
        if is_llm:
            # Special case for opencode idle state (as a fallback)
            # Only treat as green if it is JUST the prompt, not with a task attached.
            if "opencode" in cmd and pane_title.lower() in ["oc", "oc ", "oc |", "oc | "]:
                return "●", "green"

            # For known LLM tools and interpreters, treat as Green only if explicitly idle.
            if cmd in ["node", "python", "python3", "claude", "opencode"]:
                if pane_title.lower() in [w_name_lower, "node", "python", "terminal", "claude", "opencode", ""]:
                    return "●", "green"
            
            # Default for LLMs is Yellow (Working/Waiting)
            return "●", "yellow"
            
        # 4. General busy state (other non-shells)
        if cmd and cmd not in shells:
            return "●", "cyan"
            
        return "", ""

    def populate_table(self) -> None:
        """Fetch tmux data and update the table surgically."""
        if self.is_refreshing:
            return

        table = None
        try:
            table = self.query_one(DataTable)
        except Exception:
            return

        self.is_refreshing = True

        try:
            # Use direct tmux call for speed and freshness
            # Use tab as delimiter to avoid issues with pipes in titles/commands
            # Format: window_id | window_name | pane_title | pane_current_command | session_id | session_name | window_index
            res = subprocess.run(
                ["tmux", "list-windows", "-a", "-F", "#{window_id}\t#{window_name}\t#{pane_title}\t#{pane_current_command}\t#{session_id}\t#{session_name}\t#{window_index}"],
                capture_output=True, text=True, check=False
            )

            lines = res.stdout.strip().split("\n")
            raw_data = []
            for line in lines:
                if not line: continue
                parts = line.split("\t")
                if len(parts) >= 7:
                    raw_data.append({
                        "w_id": parts[0],
                        "w_name": parts[1],
                        "p_title": parts[2],
                        "p_cmd": parts[3],
                        "s_id": parts[4],
                        "s_name": parts[5],
                        "w_index": parts[6]
                    })

            # Sort by session name then window index
            def get_sort_key(x):
                try:
                    idx = int(x["w_index"])
                except (ValueError, TypeError):
                    idx = 0
                return (x["s_name"], idx)

            raw_data.sort(key=get_sort_key)

            current_row_index = table.cursor_row
            current_col_index = table.cursor_coordinate.column if table.cursor_coordinate else 1
            current_row_key = None
            if current_row_index is not None:
                try:
                    current_row_key = table.coordinate_to_cell_key(table.cursor_coordinate).row_key
                except Exception:
                    pass

            new_metadata = {}
            updated_window_ids = [] 
            last_session_id = None
            
            last_session_name = self.config.get("last_session_name")
            last_window_name = self.config.get("last_window_name")
            last_selected_id = None

            for win in raw_data:
                w_id = win["w_id"]
                s_id = win["s_id"]
                s_name = win["s_name"]
                raw_w_name = win["w_name"]
                
                updated_window_ids.append(w_id)
                display_session_name = escape(s_name) if s_id != last_session_id else ""
                last_session_id = s_id

                if self.first_load and s_name == last_session_name and raw_w_name == last_window_name:
                    last_selected_id = w_id

                status_icon, status_color = self.get_window_status(raw_w_name, win["p_title"], win["p_cmd"])
                
                # Check for notification trigger
                if w_id in self.notify_enabled_windows and not self.first_load:
                    old_meta = self.row_metadata.get(w_id, {})
                    old_color = old_meta.get("status_color")
                    # Trigger if we transitioned from something else TO green
                    if old_color and old_color != "green" and status_color == "green":
                        self.trigger_notification(raw_w_name, s_name)

                new_metadata[w_id] = {
                    "session_id": s_id,
                    "window_id": w_id,
                    "session_name": s_name,
                    "display_session_name": display_session_name,
                    "window_name": raw_w_name,
                    "display_window_name": escape(raw_w_name),
                    "status_icon": status_icon,
                    "status_color": status_color
                }

            if self.first_load:
                if last_selected_id:
                    self.target_window_after_refresh = last_selected_id
                self.first_load = False

            # Detect new windows
            if self.row_metadata:
                new_ids = set(new_metadata.keys()) - set(self.row_metadata.keys())
                if new_ids and not hasattr(self, 'target_window_after_refresh'):
                    self.target_window_after_refresh = list(new_ids)[0]

            # Clear and repopulate if the set of windows or their order changed
            current_keys = [str(k.value) for k in table.rows.keys()]
            if current_keys != updated_window_ids:
                table.clear()
                for w_id in updated_window_ids:
                    meta = new_metadata[w_id]
                    st_text = Text(meta["status_icon"], style=meta["status_color"]) if meta["status_icon"] else Text("")
                    if w_id in self.notify_enabled_windows:
                        st_text.append("🔔", style="white")
                    table.add_row(st_text, meta["display_session_name"], meta["display_window_name"], key=w_id)
            else:
                # Just update cells if the order is the same
                column_keys = list(table.columns.keys())
                for w_id in updated_window_ids:
                    meta = new_metadata[w_id]
                    old_meta = self.row_metadata.get(w_id, {})
                    
                    notify_changed = (w_id in self.notify_enabled_windows) != (w_id in getattr(self, "old_notify_enabled_windows", set()))
                    
                    if (old_meta.get("display_session_name") != meta["display_session_name"] or 
                        old_meta.get("display_window_name") != meta["display_window_name"] or
                        old_meta.get("status_icon") != meta["status_icon"] or
                        old_meta.get("status_color") != meta["status_color"] or
                        notify_changed):
                        
                        st_text = Text(meta["status_icon"], style=meta["status_color"]) if meta["status_icon"] else Text("")
                        if w_id in self.notify_enabled_windows:
                            st_text.append("🔔", style="white")
                        table.update_cell(w_id, column_keys[0], st_text)
                        table.update_cell(w_id, column_keys[1], meta["display_session_name"])
                        table.update_cell(w_id, column_keys[2], meta["display_window_name"])

            self.old_notify_enabled_windows = set(self.notify_enabled_windows)
            self.row_metadata = new_metadata

            # Restore selection
            target_to_move = None
            if hasattr(self, 'target_window_after_refresh') and self.target_window_after_refresh in self.row_metadata:
                target_to_move = self.target_window_after_refresh
                delattr(self, 'target_window_after_refresh')
            
            if target_to_move:
                try:
                    table.move_cursor(row=table.get_row_index(target_to_move), column=current_col_index)
                except Exception:
                    pass
            elif current_row_key and current_row_key.value in self.row_metadata:
                try:
                    table.move_cursor(row=table.get_row_index(current_row_key.value), column=current_col_index)
                except Exception:
                    pass
            elif current_row_index is not None:
                new_row_count = len(table.rows)
                if new_row_count > 0:
                    new_index = min(current_row_index, new_row_count - 1)
                    table.move_cursor(row=new_index, column=current_col_index)
            elif updated_window_ids:
                table.move_cursor(row=0, column=current_col_index)

        except Exception as e:
            self.title = f"Tmux Session Manager (Error: {e})"
        finally:
            self.is_refreshing = False
            if table is not None and table.cursor_row is not None:
                try:
                    row_key = table.coordinate_to_cell_key(table.cursor_coordinate).row_key
                    metadata = self.row_metadata.get(row_key.value)
                    if metadata:
                        target = metadata["window_id"]
                        if target != self.last_switched_target:
                            self.switch_tmux_target(target)
                            self.last_switched_target = target
                except Exception:
                    pass

    @on(DataTable.CellHighlighted)
    def handle_cell_highlighted(self, event: DataTable.CellHighlighted) -> None:
        """Switch active tmux target when a cell is highlighted."""
        if self.is_refreshing or event.cell_key.row_key is None:
            return

        metadata = self.row_metadata.get(event.cell_key.row_key.value)
        if not metadata:
            return

        session_id = metadata["session_id"]
        window_id = metadata["window_id"]
        self.session_last_window[session_id] = window_id

        target = window_id
        if target and target != self.last_switched_target:
            self.switch_tmux_target(target)
            self.last_switched_target = target

    @work(thread=True)
    def switch_tmux_target(self, target: str) -> None:
        """Switch active external tmux clients to the specified window target using fast shell commands."""
        try:
            subprocess.run(
                f"tmux list-clients -F '#{{client_name}}' | xargs -I{{}} tmux switch-client -c {{}} -t {target}",
                shell=True,
                check=False,
                capture_output=True
            )
        except Exception:
            pass

if __name__ == "__main__":
    app = TmuxSessionManagerApp()
    app.run()
