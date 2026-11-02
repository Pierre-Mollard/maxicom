//! Path helpers shared by the config, the commands, the captures and the UI: `~` for the
//! home folder both ways, and the missing folders of a file to write

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Creates the missing folders of `path`, so a file can be written there
pub fn create_parent_dirs(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

/// The path with the home folder written `~`, for a short display
pub fn shorten_home(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// `~` at the start of a path is the home folder, as in a shell
pub fn expand_home(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), dirs::home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `~` is the home folder only as the first part of a path.
    #[test]
    fn home_expansion() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            expand_home(Path::new("~/logs/x.log")),
            home.join("logs/x.log")
        );
        assert_eq!(expand_home(Path::new("a/~/x")), Path::new("a/~/x"));
        assert_eq!(expand_home(Path::new("~x")), Path::new("~x"));
    }

    /// MAX-163: the home folder is written `~`, a path out of it as it is.
    #[test]
    fn home_shortened() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            shorten_home(&home.join(".config/maxicom")),
            "~/.config/maxicom"
        );
        assert_eq!(shorten_home(&home), "~");
        assert_eq!(shorten_home(Path::new("/etc/x")), "/etc/x");
    }
}
