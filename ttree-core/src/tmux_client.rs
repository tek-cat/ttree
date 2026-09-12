use anyhow::Result;
use tokio::process::Command;

pub struct Tmux;

impl Tmux {
    /// Everything the tree needs, in a single tmux invocation.
    ///
    /// The three queries used to be three processes every 200ms, which is a lot
    /// of forking to keep up forever on a phone over a VPN. tmux takes several
    /// commands in one run, so we tag each line with its kind and split the
    /// output back apart. Returns (sessions, windows, panes, clients); a failure
    /// anywhere leaves the corresponding list empty, exactly as the separate
    /// calls did. Clients ride along because the preview loop needs them every
    /// tick too, and that was another process per tick on its own.
    pub async fn snapshot() -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
        let output = Command::new("tmux")
            .args([
                "list-sessions", "-F",
                "S#{session_id}\u{001F}#{session_name}\u{001F}#{session_last_attached}\u{001F}#{session_attached}",
                ";",
                "list-windows", "-a", "-F",
                "W#{window_id}\u{001F}#{session_id}\u{001F}#{window_name}\u{001F}#{window_active}\u{001F}#{window_width}\u{001F}#{window_height}",
                ";",
                "list-panes", "-a", "-F",
                "P#{pane_id}\u{001F}#{window_id}\u{001F}#{pane_title}\u{001F}#{pane_current_command}\u{001F}#{pane_active}\u{001F}#{pane_left}\u{001F}#{pane_top}\u{001F}#{pane_width}\u{001F}#{pane_height}",
                ";",
                "list-clients", "-F",
                "C#{client_pid} #{session_id} #{pane_id} #{client_tty}",
            ])
            .output()
            .await;

        let mut sessions = Vec::new();
        let mut windows = Vec::new();
        let mut panes = Vec::new();
        let mut clients = Vec::new();
        if let Ok(output) = output {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let Some(kind) = line.chars().next() else { continue };
                let rest = line[kind.len_utf8()..].to_string();
                match kind {
                    'S' => sessions.push(rest),
                    'W' => windows.push(rest),
                    'P' => panes.push(rest),
                    'C' => clients.push(rest),
                    _ => {}
                }
            }
        }
        (sessions, windows, panes, clients)
    }

    #[allow(dead_code)]
    pub async fn list_sessions() -> Result<Vec<String>> {
        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_id}\u{001F}#{session_name}\u{001F}#{session_last_attached}\u{001F}#{session_attached}"])
            .output()
            .await?;
        if !output.status.success() {
            return Err(anyhow::anyhow!("Failed to list sessions"));
        }
        Ok(String::from_utf8_lossy(&output.stdout).lines().map(|s| s.to_string()).collect())
    }

    #[allow(dead_code)]
    pub async fn list_windows() -> Result<Vec<String>> {
        let output = Command::new("tmux")
            .args(["list-windows", "-a", "-F", "#{window_id}\u{001F}#{session_id}\u{001F}#{window_name}\u{001F}#{window_active}\u{001F}#{window_width}\u{001F}#{window_height}"])
            .output()
            .await?;
        Ok(String::from_utf8_lossy(&output.stdout).lines().map(|s| s.to_string()).collect())
    }

    #[allow(dead_code)]
    pub async fn list_panes() -> Result<Vec<String>> {
        let output = Command::new("tmux")
            .args(["list-panes", "-a", "-F", "#{pane_id}\u{001F}#{window_id}\u{001F}#{pane_title}\u{001F}#{pane_current_command}\u{001F}#{pane_active}\u{001F}#{pane_left}\u{001F}#{pane_top}\u{001F}#{pane_width}\u{001F}#{pane_height}"])
            .output()
            .await?;
        Ok(String::from_utf8_lossy(&output.stdout).lines().map(|s| s.to_string()).collect())
    }

    /// Read a single global tmux option value (`show-options -gv <name>`),
    /// falling back to window scope (`-gwv`) for options like
    /// `window-status-current-style` that older tmux exposes only there.
    /// Returns `None` on any failure or an empty value.
    pub async fn show_option_global(name: &str) -> Option<String> {
        for scope in ["-gv", "-gwv"] {
            let output =
                Command::new("tmux").args(["show-options", scope, name]).output().await.ok()?;
            if output.status.success() {
                let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
        None
    }

    /// The session a pane belongs to, used to recognise the session ttree is
    /// itself running in. `None` if tmux can't resolve the pane.
    pub async fn session_of_pane(pane_id: &str) -> Option<String> {
        let output = Command::new("tmux")
            .args(["display-message", "-p", "-t", pane_id, "#{session_id}"])
            .output()
            .await
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if val.is_empty() {
            None
        } else {
            Some(val)
        }
    }

    #[allow(dead_code)]
    pub async fn capture_pane(pane_id: &str) -> Result<String> {
        let output =
            Command::new("tmux").args(["capture-pane", "-p", "-t", pane_id]).output().await?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
