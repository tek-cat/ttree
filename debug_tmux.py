import libtmux
import json

server = libtmux.Server()
data = []

for session in server.sessions:
    for window in session.windows:
        w_id = str(window.id)
        w_name = str(getattr(window, 'name', ''))
        
        try:
            pane = window.active_pane
            pane_id = str(pane.id)
            pane_title = str(getattr(pane, 'pane_title', ''))
            pane_cmd = str(getattr(pane, 'pane_current_command', ''))
            
            data.append({
                "window_id": w_id,
                "window_name": w_name,
                "pane_id": pane_id,
                "pane_title": pane_title,
                "pane_title_repr": repr(pane_title),
                "pane_current_command": pane_cmd
            })
        except Exception as e:
            data.append({
                "window_id": w_id,
                "window_name": w_name,
                "error": str(e)
            })

print(json.dumps(data, indent=2))
