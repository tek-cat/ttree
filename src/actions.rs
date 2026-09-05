use anyhow::{anyhow, Result};
use tokio::process::Command;

/// Mutating tmux commands: everything that creates, renames or destroys a
/// session, window or pane. Read-only queries stay in `tmux_client.rs`; keeping
/// the destructive calls together means the target guard below only has to be
/// audited in one file.
pub struct Actions;

/// Reject anything that is not a literal tmux object id of the expected kind.
///
/// tmux target strings are ambiguous by design: `-t work` is a *name* lookup,
/// and a name that happens to look like an index or that matches by prefix can
/// resolve to an object the user never selected. Worse, the sigil decides the
/// kind, so `kill-session -t @7` is accepted and kills the whole session that
/// window belongs to (verified against tmux 3.7). Since a kill is
/// unrecoverable, every id-taking function refuses to run at all unless the
/// argument is the exact form tmux hands back from `#{session_id}` and friends:
/// the right sigil followed by a decimal number, `$3` / `@7` / `%12`. A name,
/// an empty string, or a bare sigil is an error, not a command.
fn check_id(id: &str, sigil: char, kind: &str) -> Result<()> {
    let ok = id
        .strip_prefix(sigil)
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()));
    if ok {
        Ok(())
    } else {
        Err(anyhow!("expected a tmux {kind} id like `{sigil}3`, got {id:?}"))
    }
}

/// Names come straight from user typing, so spaces and punctuation have to
/// survive: they are safe because the name is passed as its own argv element
/// and ttree never goes through a shell. A newline or a NUL is different. A NUL
/// cannot be carried in an argv element at all (the spawn would fail with an
/// opaque OS error), and a newline would land in tmux's own status/format
/// output as a second line, so both are rejected up front with a message the
/// caller can show. Carriage return is rejected for the same display reason.
fn check_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("name must not be empty"));
    }
    if name.contains(['\n', '\r', '\0']) {
        return Err(anyhow!("name must not contain newlines or NUL"));
    }
    Ok(())
}

/// Run tmux with a fully pre-split argument list and return trimmed stdout.
///
/// Every element is passed as its own argv entry: no shell, no quoting, no
/// interpolation of ids or names into a command string, so nothing a user types
/// can ever become a separate word or a flag to a later command. On failure the
/// error carries tmux's own stderr ("can't find session: $9"), which is more
/// useful in the command bar than a generic message.
async fn run(args: &[&str]) -> Result<String> {
    let output = Command::new("tmux").args(args).output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        let detail = if detail.is_empty() { "command failed" } else { detail };
        return Err(anyhow!("tmux {}: {detail}", args.first().copied().unwrap_or("?")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

impl Actions {
    pub async fn kill_session(session_id: &str) -> Result<()> {
        check_id(session_id, '$', "session")?;
        run(&["kill-session", "-t", session_id]).await?;
        Ok(())
    }

    pub async fn kill_window(window_id: &str) -> Result<()> {
        check_id(window_id, '@', "window")?;
        run(&["kill-window", "-t", window_id]).await?;
        Ok(())
    }

    pub async fn kill_pane(pane_id: &str) -> Result<()> {
        check_id(pane_id, '%', "pane")?;
        run(&["kill-pane", "-t", pane_id]).await?;
        Ok(())
    }

    /// Create a window in `session_id` and return its `@id`.
    ///
    /// A session id as the target means "this session, next free index", so the
    /// new window lands at the end instead of colliding with an existing index.
    /// `-P -F` makes tmux print the id it just assigned, which is the only
    /// race-free way to learn it: a follow-up `list-windows` could pick up a
    /// window created by somebody else in the meantime.
    pub async fn new_window(session_id: &str) -> Result<String> {
        check_id(session_id, '$', "session")?;
        let id = run(&["new-window", "-t", session_id, "-P", "-F", "#{window_id}"]).await?;
        // Guard the answer too, so a garbled reply cannot become a selection
        // that later gets passed back to a kill.
        check_id(&id, '@', "window")?;
        Ok(id)
    }

    /// Split `pane_id` and return the new `%id`. `vertical` describes the split
    /// the way a user sees it (a new pane stacked below), which is tmux's `-v`;
    /// `-h` puts the new pane beside it.
    pub async fn split_pane(pane_id: &str, vertical: bool) -> Result<String> {
        check_id(pane_id, '%', "pane")?;
        let direction = if vertical { "-v" } else { "-h" };
        let id = run(&["split-window", direction, "-t", pane_id, "-P", "-F", "#{pane_id}"]).await?;
        check_id(&id, '%', "pane")?;
        Ok(id)
    }

    pub async fn rename_window(window_id: &str, name: &str) -> Result<()> {
        check_id(window_id, '@', "window")?;
        check_name(name)?;
        // The name goes last, as its own argv element. A name starting with `-`
        // is left for tmux to reject as an unknown flag: that is a loud, safe
        // failure, and it beats slipping in a `--` terminator that older tmux
        // builds might store as the literal new name.
        run(&["rename-window", "-t", window_id, name]).await?;
        Ok(())
    }

    pub async fn rename_session(session_id: &str, name: &str) -> Result<()> {
        check_id(session_id, '$', "session")?;
        check_name(name)?;
        run(&["rename-session", "-t", session_id, name]).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_real_ids() {
        assert!(check_id("$3", '$', "session").is_ok());
        assert!(check_id("@7", '@', "window").is_ok());
        assert!(check_id("%12", '%', "pane").is_ok());
        assert!(check_id("$1234567", '$', "session").is_ok());
        assert!(check_id("$0", '$', "session").is_ok());
    }

    #[test]
    fn rejects_names_and_empty() {
        assert!(check_id("", '$', "session").is_err());
        assert!(check_id("work", '$', "session").is_err());
        assert!(check_id("my-session", '$', "session").is_err());
        assert!(check_id("0", '$', "session").is_err());
        assert!(check_id("ttree:1.0", '@', "window").is_err());
    }

    #[test]
    fn rejects_sigil_without_digits() {
        assert!(check_id("$", '$', "session").is_err());
        assert!(check_id("$abc", '$', "session").is_err());
        assert!(check_id("$3x", '$', "session").is_err());
        assert!(check_id("$ 3", '$', "session").is_err());
    }

    /// The sigil is what tells tmux which kind of object a target is, so a pane
    /// or window id handed to a session call would silently hit the enclosing
    /// session. That must not be a successful command.
    #[test]
    fn rejects_wrong_kind_of_id() {
        assert!(check_id("@7", '$', "session").is_err());
        assert!(check_id("%12", '$', "session").is_err());
        assert!(check_id("$3", '@', "window").is_err());
        assert!(check_id("@7", '%', "pane").is_err());
    }

    #[test]
    fn id_check_handles_multibyte_input() {
        // Slicing by byte would have panicked here; the check must just say no.
        assert!(check_id("→", '$', "session").is_err());
        assert!(check_id("séance", '$', "session").is_err());
    }

    #[test]
    fn accepts_names_with_spaces_and_punctuation() {
        assert!(check_name("my project").is_ok());
        assert!(check_name("build: release (v2)").is_ok());
        assert!(check_name("café & tea").is_ok());
    }

    #[test]
    fn rejects_newline_and_nul_names() {
        assert!(check_name("two\nlines").is_err());
        assert!(check_name("trailing\n").is_err());
        assert!(check_name("carriage\rreturn").is_err());
        assert!(check_name("nul\0byte").is_err());
        assert!(check_name("").is_err());
    }
}
