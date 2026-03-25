import subprocess
import json
import os

def get_tmux_data():
    # Format: window_id | window_name | pane_title | pane_current_command | session_name | pane_pid
    res = subprocess.run(
        ["tmux", "list-panes", "-a", "-F", "#{window_id}\t#{window_name}\t#{pane_title}\t#{pane_current_command}\t#{session_name}\t#{pane_pid}"],
        capture_output=True, text=True, check=False
    )

    lines = res.stdout.strip().split("\n")
    data = []
    for line in lines:
        if not line: continue
        parts = line.split("\t")
        if len(parts) >= 6:

            title = parts[2]
            data.append({
                "window_id": parts[0],
                "window_name": parts[1],
                "pane_title": title,
                "pane_title_repr": repr(title),
                "pane_current_command": parts[3],
                "session_name": parts[4],
                "pane_pid": parts[5]
            })
    return data

if __name__ == "__main__":
    print(json.dumps(get_tmux_data(), indent=2))
