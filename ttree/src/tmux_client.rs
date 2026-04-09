use anyhow::Result;
use tokio::process::Command;

pub struct Tmux;

impl Tmux {
    pub async fn list_sessions() -> Result<Vec<String>> {
        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_id}\u{001F}#{session_name}\u{001F}#{session_last_attached}\u{001F}#{session_attached}"])
            .output()
            .await?;
        if !output.status.success() {
            return Err(anyhow::anyhow!("Failed to list sessions"));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect())
    }

    pub async fn list_windows() -> Result<Vec<String>> {
        let output = Command::new("tmux")
            .args(["list-windows", "-a", "-F", "#{window_id}\u{001F}#{session_id}\u{001F}#{window_name}\u{001F}#{window_active}\u{001F}#{window_width}\u{001F}#{window_height}"])
            .output()
            .await?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect())
    }

    pub async fn list_panes() -> Result<Vec<String>> {
        let output = Command::new("tmux")
            .args(["list-panes", "-a", "-F", "#{pane_id}\u{001F}#{window_id}\u{001F}#{pane_title}\u{001F}#{pane_current_command}\u{001F}#{pane_active}\u{001F}#{pane_left}\u{001F}#{pane_top}\u{001F}#{pane_width}\u{001F}#{pane_height}"])
            .output()
            .await?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect())
    }

    #[allow(dead_code)]
    pub async fn capture_pane(pane_id: &str) -> Result<String> {
        let output = Command::new("tmux")
            .args(["capture-pane", "-p", "-t", pane_id])
            .output()
            .await?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
