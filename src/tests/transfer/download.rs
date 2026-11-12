use std::time::UNIX_EPOCH;

use super::*;

/// A fresh folder in the temp dir, for one test
fn folder(test: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!(
        "maxicom_test_{}_download_{test}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&folder);
    folder
}

fn file(name: &str, size: Option<u64>, mtime: Option<u64>) -> Event {
    Event::File {
        name: name.to_string(),
        size,
        mtime,
    }
}

/// A file is written as `.part`, then renamed and dated when complete; the folder is made
/// when the first file comes.
#[test]
fn part_then_renamed_and_dated() {
    let folder = folder("part");
    let mut download = Download::new(folder.clone(), None);
    download
        .apply(file("fw.bin", Some(5), Some(1_000_000)))
        .unwrap();
    download.apply(Event::Data(b"hello".to_vec())).unwrap();
    assert_eq!(
        download.partial(),
        Some(folder.join("fw.bin.part").as_path())
    );
    assert_eq!(download.name().as_deref(), Some("fw.bin"));
    assert!(!folder.join("fw.bin").exists());

    download.apply(Event::FileEnd).unwrap();
    assert_eq!(fs::read(folder.join("fw.bin")).unwrap(), b"hello");
    assert!(!folder.join("fw.bin.part").exists());
    let modified = fs::metadata(folder.join("fw.bin"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        modified.duration_since(UNIX_EPOCH).unwrap().as_secs(),
        1_000_000
    );
    assert_eq!(download.take_received(), [(folder.join("fw.bin"), 5)]);
    assert_eq!(download.take_received(), []);
    assert_eq!((download.count(), download.partial()), (1, None));
    let _ = fs::remove_dir_all(&folder);
}

/// A name taken, as a file or a `.part`, gets `.1`, `.2`...: nothing is overwritten.
#[test]
fn never_overwrites() {
    let folder = folder("names");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("fw.bin"), b"old").unwrap();
    fs::write(folder.join("fw.bin.1.part"), b"left over").unwrap();
    let mut download = Download::new(folder.clone(), None);
    for _ in 0..2 {
        download.apply(file("fw.bin", None, None)).unwrap();
        download.apply(Event::Data(b"new".to_vec())).unwrap();
        download.apply(Event::FileEnd).unwrap();
    }
    let paths: Vec<_> = download
        .take_received()
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert_eq!(paths, [folder.join("fw.bin.2"), folder.join("fw.bin.3")]);
    assert_eq!(fs::read(folder.join("fw.bin")).unwrap(), b"old");
    assert_eq!(
        fs::read(folder.join("fw.bin.1.part")).unwrap(),
        b"left over"
    );
    let _ = fs::remove_dir_all(&folder);
}

/// XMODEM sends no name: its file is the one given, opened at the first data; an XMODEM
/// file with no data is written empty.
#[test]
fn xmodem_file() {
    let folder = folder("xmodem");
    let mut download = Download::new(folder.clone(), Some("dump.bin".to_string()));
    assert_eq!(download.name().as_deref(), Some("dump.bin"));
    download.apply(Event::Data(b"abc".to_vec())).unwrap();
    download.apply(Event::FileEnd).unwrap();
    assert_eq!(fs::read(folder.join("dump.bin")).unwrap(), b"abc");

    let mut empty = Download::new(folder.clone(), Some("empty.bin".to_string()));
    empty.apply(Event::FileEnd).unwrap();
    assert_eq!(fs::read(folder.join("empty.bin")).unwrap(), b"");
    let _ = fs::remove_dir_all(&folder);
}

/// A download stopped in a file keeps its `.part`, with what came.
#[test]
fn stopped_keeps_the_part() {
    let folder = folder("stopped");
    let mut download = Download::new(folder.clone(), None);
    download.apply(file("big.bin", Some(10), None)).unwrap();
    download.apply(Event::Data(b"half".to_vec())).unwrap();
    drop(download);
    assert_eq!(fs::read(folder.join("big.bin.part")).unwrap(), b"half");
    let _ = fs::remove_dir_all(&folder);
}
