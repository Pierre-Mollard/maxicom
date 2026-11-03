use super::*;

fn check(toml: &str) -> Result<LinksConfig, String> {
    let config: LinksConfig = toml::from_str(toml).map_err(|e| e.to_string())?;
    config.check().map_err(|e| e.to_string())?;
    Ok(config)
}

/// MAX-185: the ticket's example reads in the file's order, the line fields optional,
/// and writes back the same, the missing fields left out.
/// How: a serial link with all its fields, one on another machine with the same name and
/// its address only, a TCP link; parsed, checked, then written and parsed again.
#[test]
fn format_round_trips() {
    let text = r#"
            [[link]]
            name = "esp32"
            hosts = ["laptop"]
            address = "/dev/ttyUSB0"
            baud = 115200
            bits = 8
            parity = "none"
            stop_bits = 1
            flow = "none"

            [[link]]
            name = "esp32"
            hosts = ["LAB-PC"]
            address = "COM3"

            [[link]]
            name = "router console"
            address = "tcp://192.168.1.50:4000"
        "#;
    let config = check(text).unwrap();
    let names: Vec<_> = (config.links.iter())
        .map(|link| (link.name.as_str(), link.address.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("esp32", "/dev/ttyUSB0"),
            ("esp32", "COM3"),
            ("router console", "tcp://192.168.1.50:4000")
        ]
    );
    assert_eq!(config.links[0].flow, Some(FlowControl::None));
    assert_eq!(config.links[1].baud, None);

    let written = toml::to_string(&config).unwrap();
    assert_eq!(toml::from_str::<LinksConfig>(&written).unwrap(), config);
    let com3 = written.split("[[link]]").nth(2).unwrap();
    assert_eq!(
        com3.trim(),
        "name = \"esp32\"\nhosts = [\"LAB-PC\"]\naddress = \"COM3\""
    );
    assert_eq!(check("").unwrap(), LinksConfig::default());
}

/// MAX-185: a link without `hosts` is for every machine, else for the ones named,
/// ignoring case; an unknown machine has the links without `hosts` only.
#[test]
fn hosts_pick_the_machines() {
    let link = |hosts: &[&str]| SavedLink {
        hosts: hosts.iter().map(|host| host.to_string()).collect(),
        ..SavedLink::default()
    };
    assert!(link(&[]).is_for(Some("laptop")));
    assert!(link(&[]).is_for(None));
    assert!(link(&["laptop", "LAB-PC"]).is_for(Some("lab-pc")));
    assert!(!link(&["laptop"]).is_for(Some("lab-pc")));
    assert!(!link(&["laptop"]).is_for(None));
}

/// MAX-185: a missing line field is `[link]`'s.
#[test]
fn line_fields_over_the_defaults() {
    let defaults = LinkDefaults {
        baud: 9600,
        parity: Parity::Even,
        ..LinkDefaults::default()
    };
    let link = SavedLink {
        address: "COM3".into(),
        baud: Some(57600),
        ..SavedLink::default()
    };
    let line = link.over(&defaults);
    assert_eq!(line.device, "COM3");
    assert_eq!(line.baud, 57600);
    assert_eq!(line.parity, Parity::Even);
    assert_eq!(line.bits, 8);
}

/// MAX-185: the check refuses an empty name or host, an address that doesn't parse, line
/// settings on a TCP link, line fields out of range, a misspelt field, and a name twice
/// on one machine (the same hosts, a shared one, or no `hosts` on either); the same name
/// on two machines passes. Each error says which link, by number and name.
#[test]
fn bad_links() {
    let one = |fields: &str| check(&format!("[[link]]\n{fields}\n")).unwrap_err();
    assert_eq!(
        one("name = ' '\naddress = 'COM3'"),
        "link #1 ' ': the name is empty"
    );
    assert_eq!(
        one("name = 'a'\nhosts = ['']\naddress = 'COM3'"),
        "link #1 'a': a host is empty"
    );
    assert_eq!(
        one("name = 'a'\naddress = 'tcp://nope'"),
        "link #1 'a': 'nope' is not host:port"
    );
    assert_eq!(
        one("name = 'a'\naddress = 'tcp://h:1'\nbaud = 9600"),
        "link #1 'a': a TCP link has no line settings: its bridge sets the line"
    );
    assert_eq!(
        one("name = 'a'\naddress = 'COM3'\nbits = 9"),
        "link #1 'a': bits is not a value between 5 and 8"
    );
    assert!(one("name = 'a'\naddress = 'COM3'\nbaudrate = 9600").contains("unknown field"));

    let two = |first: &str, second: &str| {
        check(&format!(
            "[[link]]\nname = 'a'\naddress = 'COM1'\n{first}\n\
                 [[link]]\nname = 'a'\naddress = 'COM2'\n{second}\n"
        ))
    };
    let twice = "link #2 'a': link #1 has this name on the same machine".to_string();
    assert_eq!(two("", ""), Err(twice.clone()));
    assert_eq!(two("hosts = ['x']", ""), Err(twice.clone()));
    assert_eq!(two("hosts = ['x', 'y']", "hosts = ['Y']"), Err(twice));
    assert!(two("hosts = ['x']", "hosts = ['y']").is_ok());
}

/// MAX-185: no file is no link, nothing written; a bad file says where and which link.
/// How: a temp folder without the file, then with a bad link in it.
#[test]
fn load_says_where() {
    let dir = std::env::temp_dir().join(format!("maxicom_links_{}", std::process::id()));
    let path = dir.join("links.toml");
    assert_eq!(LinksConfig::load(&path).unwrap(), LinksConfig::default());
    assert!(!path.exists());

    fs::create_dir_all(&dir).unwrap();
    fs::write(&path, "[[link]]\nname = 'a'\naddress = ''\n").unwrap();
    let err = LinksConfig::load(&path).unwrap_err().to_string();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        err,
        format!(
            "invalid link in '{}': link #1 'a': the device is empty",
            path.display()
        )
    );
}

/// MAX-185: a serial port is saved for this machine with all its line settings, a TCP
/// link for every machine without any; the names offered are the device's file name and
/// the TCP link's `host:port`.
#[test]
fn saved_from_a_config() {
    let mut serial = LinkConfig {
        address: "/dev/ttyUSB0".parse().unwrap(),
        ..LinkConfig::default()
    };
    serial.line.baud = 9600;
    serial.line.framing = "7E2".parse().unwrap();
    let link = SavedLink::from_config("esp32".into(), &serial, Some("laptop"));
    let expected = SavedLink {
        name: "esp32".into(),
        hosts: vec!["laptop".into()],
        address: "/dev/ttyUSB0".into(),
        baud: Some(9600),
        bits: Some(7),
        parity: Some(Parity::Even),
        stop_bits: Some(2),
        flow: Some(FlowControl::None),
    };
    assert_eq!(link, expected);
    assert_eq!(link.over(&LinkDefaults::default()).line(), serial.line);

    let tcp = LinkConfig {
        address: "tcp://10.0.0.1:4000".parse().unwrap(),
        ..serial
    };
    let link = SavedLink::from_config("router".into(), &tcp, Some("laptop"));
    assert!(link.hosts.is_empty() && !link.has_line_fields());

    assert_eq!(default_name(&serial.address), "ttyUSB0");
    assert_eq!(default_name(&"COM3".parse().unwrap()), "COM3");
    assert_eq!(default_name(&tcp.address), "10.0.0.1:4000");
}

/// MAX-185: a save reads the file again first, so a link written meanwhile stays; the
/// file is written whole with its header, no temp file left; a name taken on this machine
/// saves nothing; an edit replaces its link in place, unless it changed meanwhile; a
/// delete takes it out, unless it's gone; a file that doesn't load isn't written over.
/// How: a temp folder, a link added to no file, another written in by hand, a second link
/// added, the same name again, then a broken file.
#[test]
fn add_reads_again_and_writes_whole() {
    let dir = std::env::temp_dir().join(format!("maxicom_links_add_{}", std::process::id()));
    let path = dir.join("sub").join("links.toml");
    let link = |name: &str, address: &str| SavedLink {
        name: name.into(),
        address: address.into(),
        ..SavedLink::default()
    };
    let added = LinksConfig::put(&path, link("a", "COM1"), None).unwrap();
    assert_eq!(added.links.len(), 1);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with(HEADER), "{text}");

    fs::write(
        &path,
        format!("{text}\n[[link]]\nname = 'by hand'\naddress = 'COM2'\n"),
    )
    .unwrap();
    let added = LinksConfig::put(&path, link("b", "COM3"), None).unwrap();
    let names: Vec<_> = added.links.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["a", "by hand", "b"]);
    assert_eq!(LinksConfig::load(&path).unwrap(), added);
    let left: Vec<_> = (fs::read_dir(path.parent().unwrap()).unwrap())
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["links.toml"]);

    let taken = LinksConfig::put(&path, link("a", "COM4"), None).unwrap_err();
    assert_eq!(taken, "link #1 has this name on the same machine");
    assert_eq!(LinksConfig::load(&path).unwrap(), added);

    // an edit replaces the link in its place; one changed meanwhile saves nothing
    let edited = LinksConfig::put(&path, link("by hand 2", "COM6"), Some(&added.links[1]));
    let names: Vec<_> = (edited.unwrap().links.iter())
        .map(|l| (l.name.clone(), l.address.clone()))
        .collect();
    assert_eq!(names[1], ("by hand 2".into(), "COM6".into()));
    let gone = LinksConfig::put(&path, link("z", "COM7"), Some(&added.links[1]));
    assert_eq!(
        gone.unwrap_err(),
        "changed in the file meanwhile: not saved"
    );

    // a delete takes the link out, the others kept in order; one gone already says so
    let left = LinksConfig::remove(&path, &added.links[0]).unwrap();
    let names: Vec<_> = left.links.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["by hand 2", "b"]);
    assert_eq!(LinksConfig::load(&path).unwrap(), left);
    let again = LinksConfig::remove(&path, &added.links[0]).unwrap_err();
    assert_eq!(again, "changed in the file meanwhile: not deleted");

    fs::write(&path, "[[link]]\nname = 'x'\n").unwrap();
    let broken = LinksConfig::put(&path, link("c", "COM5"), None).unwrap_err();
    assert!(
        broken.starts_with("not saved: invalid syntax in"),
        "{broken}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "[[link]]\nname = 'x'\n");
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-185: this machine has a name.
#[test]
fn this_host_is_known() {
    assert!(this_host().is_some_and(|name| !name.contains('\0')));
}
