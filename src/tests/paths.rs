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
