//! Where ttree's own config and state live on disk, e.g. `~/.config/ttree` on
//! Linux. Shared by every ttree binary so they agree on the same directory.

use std::path::PathBuf;

/// `None` only if the platform gives us no resolvable home/config directory.
pub fn config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "tek", "ttree")
        .map(|proj_dirs| proj_dirs.config_dir().to_path_buf())
}
