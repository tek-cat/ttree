import sys
import json
import os
import subprocess
import urllib.request
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
        Binding("d", "delete_target", "Delete Pane"),
        Binding("N", "toggle_notifications", "Toggle Alert"),
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
        # Dynamic column indices
        if self.view_mode == 0: session_col, window_col = 1, 2
        elif self.view_mode == 1: session_col, window_col = 3, 1
        else: session_col, window_col = 2, 3
        
        current_row = table.cursor_row
        if current_row is None: return
        row_keys = list(table.rows.keys())
        current_meta = self.row_metadata.get(row_keys[current_row].value)
        if not current_meta: return

        target_col = table.cursor_coordinate.column
        if target_col == session_col:
            # Find first row of NEXT session
            for r in range(current_row + 1, len(row_keys)):
                meta = self.row_metadata.get(row_keys[r].value)
                if meta and meta["session_id"] != current_meta["session_id"]:
                    # Try to restore last known window/pane for this session
                    last_w_id = self.session_last_window.get(meta["session_id"])
                    last_p_id = self.window_last_pane.get(last_w_id) if last_w_id else None
                    target_p_id = last_p_id if last_p_id in self.row_metadata else meta["pane_id"]
                    
                    table.move_cursor(row=table.get_row_index(target_p_id), column=session_col)
                    return
        elif target_col == window_col:
            # Find first row of NEXT window
            for r in range(current_row + 1, len(row_keys)):
                meta = self.row_metadata.get(row_keys[r].value)
                if meta and meta["window_id"] != current_meta["window_id"]:
                    # Try to restore last known pane for this window
                    last_p_id = self.window_last_pane.get(meta["window_id"])
                    target_p_id = last_p_id if last_p_id in self.row_metadata else meta["pane_id"]
                    
                    table.move_cursor(row=table.get_row_index(target_p_id), column=window_col)
                    return
        
        table.action_cursor_down()

    def action_cursor_up(self) -> None:
        table = self.query_one(DataTable)
        # Dynamic column indices
        if self.view_mode == 0: session_col, window_col = 1, 2
        elif self.view_mode == 1: session_col, window_col = 3, 1
        else: session_col, window_col = 2, 3
        
        current_row = table.cursor_row
        if current_row is None: return
        row_keys = list(table.rows.keys())
        current_meta = self.row_metadata.get(row_keys[current_row].value)
        if not current_meta: return

        target_col = table.cursor_coordinate.column
        if target_col == session_col:
            # Find first row of PREVIOUS session
            target_session_id = None
            for r in range(current_row - 1, -1, -1):
                meta = self.row_metadata.get(row_keys[r].value)
                if meta and meta["session_id"] != current_meta["session_id"]:
                    target_session_id = meta["session_id"]
                    break
            
            if target_session_id:
                # Need to find the first pane row of THIS specific session
                first_pane_meta = None
                for r in range(len(row_keys)):
                    meta = self.row_metadata.get(row_keys[r].value)
                    if meta and meta["session_id"] == target_session_id:
                        first_pane_meta = meta
                        break
                
                if first_pane_meta:
                    last_w_id = self.session_last_window.get(target_session_id)
                    last_p_id = self.window_last_pane.get(last_w_id) if last_w_id else None
                    target_p_id = last_p_id if last_p_id in self.row_metadata else first_pane_meta["pane_id"]
                    
                    table.move_cursor(row=table.get_row_index(target_p_id), column=session_col)
                    return
        elif target_col == window_col:
            # Find first row of PREVIOUS window
            target_window_id = None
            for r in range(current_row - 1, -1, -1):
                meta = self.row_metadata.get(row_keys[r].value)
                if meta and meta["window_id"] != current_meta["window_id"]:
                    target_window_id = meta["window_id"]
                    break
            
            if target_window_id:
                # Find first pane row of THIS specific window
                first_pane_meta = None
                for r in range(len(row_keys)):
                    meta = self.row_metadata.get(row_keys[r].value)
                    if meta and meta["window_id"] == target_window_id:
                        first_pane_meta = meta
                        break
                
                if first_pane_meta:
                    last_p_id = self.window_last_pane.get(target_window_id)
                    target_p_id = last_p_id if last_p_id in self.row_metadata else first_pane_meta["pane_id"]
                    
                    table.move_cursor(row=table.get_row_index(target_p_id), column=window_col)
                    return
        
        table.action_cursor_up()


    def action_cursor_left(self) -> None:
        self.view_mode = (self.view_mode - 1) % 3
        self.recreate_columns()

    def action_cursor_right(self) -> None:
        self.view_mode = (self.view_mode + 1) % 3
        self.recreate_columns()

    def recreate_columns(self) -> None:
        """Recreate table columns based on current preference."""
        table = self.query_one(DataTable)
        # Store current state
        cursor_row = table.cursor_row
        
        table.clear(columns=True)
        table.add_column("S", width=3)
        
        if self.view_mode == 0:
            table.add_column("Session")
            table.add_column("Window")
            table.add_column("Pane")
        elif self.view_mode == 1:
            table.add_column("Window")
            table.add_column("Pane")
            table.add_column("Session")
        else:
            table.add_column("Pane")
            table.add_column("Session")
            table.add_column("Window")
            
        table.fixed_columns = 1
        log_debug(f"Columns set (Mode {self.view_mode})")
        
        # Force a full repopulate
        self.populate_table(force_full=True)
        
        # Always restore cursor to a valid column (usually column 1)
        if cursor_row is not None:
            new_col = min(1, len(table.columns) - 1) if len(table.columns) > 0 else 0
            table.move_cursor(row=cursor_row, column=new_col)

    def __init__(self):
        super().__init__()
        self.server = libtmux.Server()
        self.is_refreshing = False
        self.last_switched_target = None
        self.session_last_window = {}
        self.window_last_pane = {}
        self.row_metadata = {}
        self.config = load_config()
        self.first_load = True
        self.notify_enabled_windows = set()
        self.view_mode = 0
        self.switch_cooldown_until = 0


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
        self.recreate_columns()
        self.set_interval(1.0, self.populate_table)
        self.query_one(DataTable).focus()

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
        """Delete the currently selected pane."""
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
                    # Kill the specific pane
                    subprocess.run(["tmux", "kill-pane", "-t", metadata["pane_id"]], check=False)
                    
                    # Explicitly refresh immediately
                    self.populate_table()
                except Exception:
                    pass

        self.push_screen(DeleteConfirmModal(f"{metadata['window_name']}:{metadata['pane_title']}"), handle_delete)

    def action_toggle_notifications(self) -> None:
        """Toggle system notifications for the currently selected pane."""
        table = self.query_one(DataTable)
        if table.cursor_row is None:
            return
        
        row_key = table.coordinate_to_cell_key(table.cursor_coordinate).row_key
        metadata = self.row_metadata.get(row_key.value)
        if not metadata:
            return

        p_id = metadata["pane_id"]
        if p_id in self.notify_enabled_windows:
            self.notify_enabled_windows.remove(p_id)
            self.trigger_notification("Notifications Disabled", f"Notifications turned OFF for pane '{metadata['pane_title']}'")
        else:
            self.notify_enabled_windows.add(p_id)
            self.trigger_notification("Notifications Enabled", f"Notifications turned ON for pane '{metadata['pane_title']}'")
        
        self.populate_table()

    @work(thread=True)
    def trigger_notification(self, title: str, message: str) -> None:
        """Send a system notification."""
        try:
            subprocess.run(
                ["notify-send", title, message],
                check=False
            )
        except:
            pass

    def get_opencode_status(self, pid: str) -> str:
        """Query the opencode local API or logs for status.
        Handles cases where pid is the shell (finds the opencode child)."""
        try:
            target_pid = pid
            # 0. Find child opencode or descendant
            try:
                # Use pgrep -P to find ALL descendants by recursively checking children
                def find_opencode(p):
                    res = subprocess.run(["pgrep", "-P", p], capture_output=True, text=True, check=False)
                    if res.returncode == 0:
                        children = res.stdout.strip().split("\n")
                        for child_pid in children:
                            with open(f"/proc/{child_pid}/comm", "r") as f:
                                comm = f.read().strip()
                                if "opencode" in comm or "oc" in comm:
                                    return child_pid
                                # If not opencode, check ITS children
                                found = find_opencode(child_pid)
                                if found: return found
                    return None
                
                # First check if the pid itself is opencode
                with open(f"/proc/{pid}/comm", "r") as f:
                    comm = f.read().strip()
                    if "opencode" in comm or "oc" in comm:
                        target_pid = pid
                    else:
                        found = find_opencode(pid)
                        if found: target_pid = found
            except:
                pass

            # 1. Try to find the specific log file via /proc/target_pid/fd
            try:
                fd_path = f"/proc/{target_pid}/fd"
                if os.path.exists(fd_path):
                    for fd in os.listdir(fd_path):
                        try:
                            fd_full_path = os.path.join(fd_path, fd)
                            link = os.readlink(fd_full_path)
                            if "opencode/log" in link and (".log" in link or "log" in link.lower()):
                                # We found the log file for THIS process
                                last_event = ""
                                # Use fd_full_path (/proc/PID/fd/N) to read even if deleted/rotated
                                with open(fd_full_path, "rb") as f:
                                    f.seek(0, os.SEEK_END)
                                    pos = f.tell()
                                    chunk_size = 4096
                                    while pos > 0 and not last_event:
                                        seek_pos = max(0, pos - chunk_size)
                                        f.seek(seek_pos)
                                        chunk = f.read(pos - seek_pos).decode("utf-8", errors="ignore")
                                        lines = chunk.split("\n")
                                        for line in reversed(lines):
                                            if "service=bus type=session.idle publishing" in line:
                                                last_event = "idle"
                                                break
                                            # Definitely working signals
                                            if any(kw in line for kw in ["type=message.part.delta", "status=started resolveTools", "stream"]):
                                                last_event = "status"
                                                break
                                            # Feedback signals
                                            if any(kw in line for kw in ["type=session.error", "type=question.asked"]):
                                                last_event = "feedback"
                                                break
                                            if "status=started question" in line or "ask_user" in line:
                                                last_event = "feedback"
                                                break
                                            # Neutral signals (often happen after idle)
                                            # type=session.status, type=session.updated, type=message.updated
                                            # We DON'T break for these, we keep looking for idle or real working signals
                                        pos = seek_pos
                                
                                if last_event:
                                    if last_event == "idle": return "green"
                                    if last_event == "status": return "yellow"
                                    if last_event == "feedback": return "red"
                        except:
                            continue
            except:
                pass

            # 2. Try API (only if target_pid is correct)
            try:
                environ_path = f"/proc/{target_pid}/environ"
                if os.path.exists(environ_path):
                    with open(environ_path, "rb") as f:
                        env_data = f.read().split(b"\0")
                    env = {}
                    for item in env_data:
                        if b"=" in item:
                            try:
                                k, v = item.split(b"=", 1)
                                env[k.decode("utf-8", errors="ignore")] = v.decode("utf-8", errors="ignore")
                            except:
                                pass
                    session_id = env.get("OPENCODE_SESSION_ID")
                    port = env.get("OPENCODE_PORT", "4096")
                    if session_id:
                        url = f"http://localhost:{port}/session/{session_id}"
                        with urllib.request.urlopen(url, timeout=0.5) as response:
                            if response.getcode() == 200:
                                data = json.loads(response.read().decode())
                                activity = data.get("activity", "").lower()
                                if any(kw in activity for kw in ["waiting", "feedback", "question", "input", "confirm", "approve", "plan"]):
                                    return "red"
                                if any(kw in activity for kw in ["thinking", "working", "processing", "analyzing", "generating", "executing"]):
                                    return "yellow"
                                return "green"
            except:
                pass

            # 3. Fallback to global log analysis (latest 3 logs)
            try:
                log_dir = os.path.expanduser("~/.local/share/opencode/log")
                if os.path.exists(log_dir):
                    logs = [os.path.join(log_dir, f) for f in os.listdir(log_dir) if f.endswith(".log")]
                    if logs:
                        # Sort by mtime descending to check newest logs first
                        logs.sort(key=lambda x: os.path.getmtime(x), reverse=True)
                        
                        for log_path in logs[:3]:  # Check up to 3 most recent logs
                            last_event = ""
                            with open(log_path, "rb") as f:
                                # Seek towards end
                                f.seek(0, os.SEEK_END)
                                pos = f.tell()
                                chunk_size = 4096
                                while pos > 0 and not last_event:
                                    seek_pos = max(0, pos - chunk_size)
                                    f.seek(seek_pos)
                                    chunk = f.read(pos - seek_pos).decode("utf-8", errors="ignore")
                                    lines = chunk.split("\n")
                                    for line in reversed(lines):
                                        if "service=bus type=session.idle publishing" in line:
                                            last_event = "idle"
                                            break
                                        if any(kw in line for kw in ["type=message.part.delta", "status=started resolveTools", "stream"]):
                                            last_event = "status"
                                            break
                                        if any(kw in line for kw in ["type=session.error", "type=question.asked"]):
                                            last_event = "feedback"
                                            break
                                        if "status=started question" in line or "ask_user" in line:
                                            last_event = "feedback"
                                            break
                                        # Neutral signals (often happen after idle)
                                        # type=session.status, type=session.updated, type=message.updated
                                        # We DON'T break for these, we keep looking for idle or real working signals
                                    pos = seek_pos
                            
                            if last_event:
                                if last_event == "idle": return "green"
                                if last_event == "status": return "yellow"
                                if last_event == "feedback": return "red"
            except:
                pass
        except:
            pass
        return ""

    def get_pane_status(self, raw_w_name: str, pane_title: str, cmd: str, pid: str = "") -> tuple[str, str]:
        """Detect status from metadata. Returns (icon, color)."""
        w_name_lower = raw_w_name.lower()
        pane_title = pane_title.strip()
        combined_title = (raw_w_name + " " + pane_title)
        combined_title_lower = combined_title.lower()
        shells = ["bash", "zsh", "fish", "sh", "tmux"]
        
        # 0. Opencode API check
        if cmd == "opencode" and pid:
            oc_status = self.get_opencode_status(pid)
            if oc_status == "red":
                return "●", "red"
            elif oc_status == "yellow":
                return "●", "yellow"
            elif oc_status == "green":
                return "●", "green"
        
        # 1. Action Required / Input Needed -> Red
        # Specifically for AI agents like Gemini and Claude
        input_required_icons = ["✋", "⏹", "❗", "⚠"]
        input_required_keywords = ["action required", "waiting for input", "confirm", "approve", "feedback", "plan ready"]
        
        if any(icon in combined_title for icon in input_required_icons) or \
           any(kw in combined_title_lower for kw in input_required_keywords):
            return "●", "red"

        # 2. Thinking/Working signals (Icons and Keywords) -> Yellow
        # These take absolute precedence after Action Required.
        working_icons = ["\u2726", "\u2802", "✦", "✸", "★", "✨", "⏳", "⌛", "🔄", "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "…"]
        working_keywords = ["thinking", "working", "processing", "running", "thought", "analyzing", "generating", 
                            "executing", "searching", "updating", "answering", "writing", "planning", "coding", 
                            "building", "testing", "creating", "fixing", "improving"]
        
        # Word-boundary-like check for keywords to avoid partial matches
        combined_words = set(combined_title_lower.replace("|", " ").replace(":", " ").replace("(", " ").replace(")", " ").split())
        
        if any(icon in combined_title for icon in working_icons) or \
           any(kw in working_keywords for kw in combined_words) or \
           "..." in combined_title:
            return "●", "yellow"

        if "waiting" in combined_title_lower and "input" not in combined_title_lower:
            return "●", "yellow"

        # 3. Shell/Idle/Explicit Ready -> Green
        ready_icons = ["\u25c7", "◇", "✓", "✔", "✅", "✳", "\u2733"]
        if cmd in shells or pane_title.lower() in shells or \
           pane_title.strip().lower() in ["$", "#", "%", ">", "oc |", "oc | "] or \
           (cmd in ["opencode", "oc"] and pane_title.startswith("OC | ")) or \
           any(icon in combined_title for icon in ready_icons) or \
           ("ready" in combined_title_lower and "not ready" not in combined_title_lower) or \
           ("waiting" in combined_title_lower and "input" in combined_title_lower):
            return "●", "green"

        # 3. Heuristic for LLM tools
        llm_tools = ["gemini", "claude", "opencode", "oc", "gpt", "anthropic", "openai", "ollama", "llama", "ai-", "assistant",
                     "aider", "continue", "supermaven", "copilot", "ghostwriter", "cursor", "deepseek", "mistral", "grok", "perplexity", "cohere",
                     "g1", "g2", "g3", "g4", "g5", "g6", "g7", "g8", "g9",
                     "c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8", "c9"]
        
        is_llm = any(tool in cmd for tool in llm_tools) or \
                 any(tool == w_name_lower or f"{tool}:" in w_name_lower or f" {tool}" in w_name_lower for tool in llm_tools)
        
        if is_llm:
            # For known LLM tools and interpreters, treat as Green only if explicitly idle.
            if cmd in ["node", "python", "python3", "claude", "opencode", "oc", "aider"]:
                if pane_title.lower() in [w_name_lower, "node", "python", "terminal", "claude", "opencode", "oc", "aider", ""]:
                    return "●", "green"
            
            # Default for LLMs is Yellow (Working/Waiting)
            return "●", "yellow"
            
        # 4. General busy state (other non-shells)
        if cmd and cmd not in shells:
            return "●", "cyan"
            
        return "", ""

    def populate_table(self, force_full: bool = False) -> None:
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
            # window_id | window_name | pane_id | pane_title | pane_current_command | session_id | session_name | window_index | pane_index | pane_pid
            res = subprocess.run(
                ["tmux", "list-panes", "-a", "-F", "#{window_id}\t#{window_name}\t#{pane_id}\t#{pane_title}\t#{pane_current_command}\t#{session_id}\t#{session_name}\t#{window_index}\t#{pane_index}\t#{pane_pid}"],
                capture_output=True, text=True, check=False
            )

            lines = res.stdout.strip().split("\n")
            log_debug(f"Panes found: {len(lines)}")
            raw_data = []
            for line in lines:
                if not line: continue
                parts = line.split("\t")
                if len(parts) >= 10:
                    raw_data.append({
                        "w_id": parts[0],
                        "w_name": parts[1],
                        "p_id": parts[2],
                        "p_title": parts[3],
                        "p_cmd": parts[4],
                        "s_id": parts[5],
                        "s_name": parts[6],
                        "w_index": parts[7],
                        "p_index": parts[8],
                        "p_pid": parts[9]
                    })

            # Sort by session name then window index then pane index
            def get_sort_key(x):
                try:
                    w_idx = int(x["w_index"])
                    p_idx = int(x["p_index"])
                except (ValueError, TypeError):
                    w_idx = 0
                    p_idx = 0
                return (x["s_name"], w_idx, p_idx)

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
            updated_pane_ids = [] 
            
            last_session_name = self.config.get("last_session_name")
            last_window_name = self.config.get("last_window_name")
            last_selected_id = None

            for pane in raw_data:
                w_id = pane["w_id"]
                s_id = pane["s_id"]
                p_id = pane["p_id"]
                s_name = pane["s_name"]
                raw_w_name = pane["w_name"]
                
                updated_pane_ids.append(p_id)
                
                # Full context on every row
                display_session_name = escape(s_name)
                display_window_name = escape(raw_w_name)

                if self.first_load and s_name == last_session_name and raw_w_name == last_window_name:
                    last_selected_id = p_id

                status_icon, status_color = self.get_pane_status(raw_w_name, pane["p_title"], pane["p_cmd"], pane["p_pid"])
                
                # Check for notification trigger
                if p_id in self.notify_enabled_windows and not self.first_load:
                    old_meta = self.row_metadata.get(p_id, {})
                    old_color = old_meta.get("status_color")
                    # Trigger if we transitioned from something else TO green (Ready)
                    if old_color and old_color != "green" and status_color == "green":
                        self.trigger_notification("Tmux Task Finished", f"Pane '{pane['p_title']}' in window '{raw_w_name}' is Ready!")
                    # Trigger if we transitioned TO red (Feedback Required)
                    elif status_color == "red" and old_color != "red":
                        self.trigger_notification("AI Feedback Required", f"Pane '{pane['p_title']}' in window '{raw_w_name}' needs input!")

                new_metadata[p_id] = {
                    "session_id": s_id,
                    "window_id": w_id,
                    "pane_id": p_id,
                    "session_name": s_name,
                    "display_session_name": display_session_name,
                    "window_name": raw_w_name,
                    "display_window_name": display_window_name,
                    "status_icon": status_icon,
                    "status_color": status_color,
                    "pane_title": pane["p_title"],
                    "display_pane_title": escape(pane["p_title"])
                }

            if self.first_load:
                if last_selected_id:
                    self.target_window_after_refresh = last_selected_id
                self.first_load = False

            # Detect new panes
            if self.row_metadata:
                new_ids = set(new_metadata.keys()) - set(self.row_metadata.keys())
                if new_ids and not hasattr(self, 'target_window_after_refresh'):
                    self.target_window_after_refresh = list(new_ids)[0]
                    import time
                    self.switch_cooldown_until = time.time() + 2.0

            # Clear and repopulate if the set of panes or their order changed, or if forced
            current_keys = [str(k.value) for k in table.rows.keys()]
            if current_keys != updated_pane_ids or force_full:
                table.clear()
                for p_id in updated_pane_ids:
                    meta = new_metadata[p_id]
                    st_text = Text(meta["status_icon"], style=meta["status_color"]) if meta["status_icon"] else Text("")
                    if p_id in self.notify_enabled_windows:
                        st_text.append("🔔", style="white")
                    
                    if self.view_mode == 0:
                        table.add_row(st_text, meta["display_session_name"], meta["display_window_name"], meta["display_pane_title"], key=p_id)
                    elif self.view_mode == 1:
                        table.add_row(st_text, meta["display_window_name"], meta["display_pane_title"], meta["display_session_name"], key=p_id)
                    else:
                        table.add_row(st_text, meta["display_pane_title"], meta["display_session_name"], meta["display_window_name"], key=p_id)
            else:
                # Just update cells if the order is the same
                column_keys = list(table.columns.keys())
                for p_id in updated_pane_ids:
                    meta = new_metadata[p_id]
                    old_meta = self.row_metadata.get(p_id, {})
                    
                    notify_changed = (p_id in self.notify_enabled_windows) != (p_id in getattr(self, "old_notify_enabled_windows", set()))
                    
                    if (old_meta.get("display_session_name") != meta["display_session_name"] or 
                        old_meta.get("display_window_name") != meta["display_window_name"] or
                        old_meta.get("display_pane_title") != meta["display_pane_title"] or
                        old_meta.get("status_icon") != meta["status_icon"] or
                        old_meta.get("status_color") != meta["status_color"] or
                        notify_changed):
                        
                        st_text = Text(meta["status_icon"], style=meta["status_color"]) if meta["status_icon"] else Text("")
                        if p_id in self.notify_enabled_windows:
                            st_text.append("🔔", style="white")
                        
                        table.update_cell(p_id, column_keys[0], st_text)
                        if self.view_mode == 0:
                            table.update_cell(p_id, column_keys[1], meta["display_session_name"])
                            table.update_cell(p_id, column_keys[2], meta["display_window_name"])
                            table.update_cell(p_id, column_keys[3], meta["display_pane_title"])
                        elif self.view_mode == 1:
                            table.update_cell(p_id, column_keys[1], meta["display_window_name"])
                            table.update_cell(p_id, column_keys[2], meta["display_pane_title"])
                            table.update_cell(p_id, column_keys[3], meta["display_session_name"])
                        else:
                            table.update_cell(p_id, column_keys[1], meta["display_pane_title"])
                            table.update_cell(p_id, column_keys[2], meta["display_session_name"])
                            table.update_cell(p_id, column_keys[3], meta["display_window_name"])


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
            elif updated_pane_ids:
                table.move_cursor(row=0, column=current_col_index)

        except Exception as e:
            self.title = f"ERR: {e}"
            log_debug(f"Populate Error: {e}")
        finally:
            self.is_refreshing = False
            import time
            if table is not None and table.cursor_row is not None and time.time() > self.switch_cooldown_until:
                try:
                    row_key = table.coordinate_to_cell_key(table.cursor_coordinate).row_key
                    metadata = self.row_metadata.get(row_key.value)
                    if metadata:
                        target = metadata["pane_id"]
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

        import time
        if time.time() < self.switch_cooldown_until:
            return

        metadata = self.row_metadata.get(event.cell_key.row_key.value)
        if not metadata:
            return

        session_id = metadata["session_id"]
        window_id = metadata["window_id"]
        pane_id = metadata["pane_id"]
        self.session_last_window[session_id] = window_id
        self.window_last_pane[window_id] = pane_id

        target = pane_id
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
