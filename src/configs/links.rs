use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    configs::LinkDefaults,
    link::{
        Address, LinkConfig, LinkKind,
        serial::{FlowControl, LineSettings, Parity},
    },
    paths::create_parent_dirs,
};

/// On top of the file the app writes: it rewrites the whole of it
const HEADER: &str = "# maxicom's saved links: written by the app, comments are lost on a save\n\n";

/// `links.toml` (MAX-185), minicom's dialing directory: the saved links, in the user's order.
/// Not created until the first save; one file can serve several machines, each link for the
/// ones its `hosts` names, every machine without it
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksConfig {
    #[serde(default, rename = "link")]
    pub links: Vec<SavedLink>,
}

/// A link as written: its name, the machines it's for, its address, and the line settings
/// that differ from `[link]`'s (none on a TCP link)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedLink {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baud: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parity: Option<Parity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_bits: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<FlowControl>,
}

impl SavedLink {
    /// `config` saved under `name`: a serial port for `host` only (a path is one machine's),
    /// with all its line settings; a TCP link for every machine, without any
    pub fn from_config(name: String, config: &LinkConfig, host: Option<&str>) -> Self {
        let address = config.address.to_string();
        if config.address.kind() == LinkKind::Tcp {
            return Self {
                name,
                address,
                ..Self::default()
            };
        }
        let link = Self {
            name,
            hosts: host.map(str::to_string).into_iter().collect(),
            address,
            ..Self::default()
        };
        link.with_line(&config.line)
    }

    /// All of `line`'s settings written on the link
    pub fn with_line(self, line: &LineSettings) -> Self {
        let [bits, _, stop_bits] = line.framing.compact_notation();
        let digit = |c: char| c.to_digit(10).map(|d| d as u8);
        Self {
            baud: Some(line.baud),
            bits: digit(bits),
            parity: Some(line.framing.parity),
            stop_bits: digit(stop_bits),
            flow: Some(line.flow_control),
            ..self
        }
    }

    /// For `host`: no `hosts` is every machine, else one of them, ignoring case (Windows
    /// names are upper case). An unknown machine has the links without `hosts` only
    pub fn is_for(&self, host: Option<&str>) -> bool {
        self.hosts.is_empty()
            || host.is_some_and(|host| self.hosts.iter().any(|h| h.eq_ignore_ascii_case(host)))
    }

    /// Whether one machine could see both links
    fn shares_a_host(&self, other: &SavedLink) -> bool {
        self.hosts.is_empty()
            || other.hosts.is_empty()
            || (self.hosts.iter()).any(|host| other.is_for(Some(host)))
    }

    /// `[link]` with this link's address and line fields over it
    pub fn over(&self, defaults: &LinkDefaults) -> LinkDefaults {
        LinkDefaults {
            device: self.address.clone(),
            baud: self.baud.unwrap_or(defaults.baud),
            bits: self.bits.unwrap_or(defaults.bits),
            parity: self.parity.unwrap_or(defaults.parity),
            stop_bits: self.stop_bits.unwrap_or(defaults.stop_bits),
            flow: self.flow.unwrap_or(defaults.flow),
        }
    }

    fn has_line_fields(&self) -> bool {
        self.baud.is_some()
            || self.bits.is_some()
            || self.parity.is_some()
            || self.stop_bits.is_some()
            || self.flow.is_some()
    }

    /// A name, hosts that aren't empty, an address that parses, line fields in range and none
    /// on a TCP link
    fn check(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("the name is empty".to_string());
        }
        if self.hosts.iter().any(|host| host.trim().is_empty()) {
            return Err("a host is empty".to_string());
        }
        let address: Address = self.address.parse()?;
        if address.kind() == LinkKind::Tcp && self.has_line_fields() {
            return Err("a TCP link has no line settings: its bridge sets the line".to_string());
        }
        // `[link]`'s checks, on this link's fields over valid ones
        let fields = self.over(&LinkDefaults::default());
        fields
            .validate()
            .map_err(|err| err.to_string().replace("link.", ""))
    }
}

impl LinksConfig {
    /// Each link checked, and no two of one machine with the same name
    pub fn check(&self) -> Result<(), SavedLinkError> {
        for (index, link) in self.links.iter().enumerate() {
            let error = |problem| SavedLinkError {
                index,
                name: link.name.clone(),
                problem,
            };
            link.check().map_err(error)?;
            let before = self.links[..index].iter().position(|other| {
                other.name.trim() == link.name.trim() && other.shares_a_host(link)
            });
            if let Some(other) = before {
                let problem = format!("link #{} has this name on the same machine", other + 1);
                return Err(error(problem));
            }
        }
        Ok(())
    }

    /// `link` added to the file as it is now (another maxicom may have changed it), or in
    /// place of `replaces` (an edit), checked, and the file written again: the links it holds
    /// then, or why nothing was saved
    pub fn put(path: &Path, link: SavedLink, replaces: Option<&SavedLink>) -> Result<Self, String> {
        let mut config = Self::load(path).map_err(|err| format!("not saved: {err}"))?;
        match replaces {
            None => config.links.push(link),
            Some(old) => match config.links.iter().position(|saved| saved == old) {
                Some(idx) => config.links[idx] = link,
                None => return Err("changed in the file meanwhile: not saved".to_string()),
            },
        }
        config.check().map_err(|err| err.problem)?;
        config
            .save(path)
            .map_err(|err| format!("could not write '{}': {err}", path.display()))?;
        Ok(config)
    }

    /// `link` taken out of the file as it is now, the file written again: the links left, or
    /// why nothing was deleted
    pub fn remove(path: &Path, link: &SavedLink) -> Result<Self, String> {
        let mut config = Self::load(path).map_err(|err| format!("not deleted: {err}"))?;
        let Some(idx) = config.links.iter().position(|saved| saved == link) else {
            return Err("changed in the file meanwhile: not deleted".to_string());
        };
        config.links.remove(idx);
        config
            .save(path)
            .map_err(|err| format!("could not write '{}': {err}", path.display()))?;
        Ok(config)
    }

    /// The whole file written to a temp file beside it, then renamed over it, so a reader
    /// never sees half of it
    fn save(&self, path: &Path) -> io::Result<()> {
        let text = toml::to_string(self).map_err(io::Error::other)?;
        create_parent_dirs(path)?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let temp = path.with_file_name(format!(".{name}.tmp"));
        fs::write(&temp, format!("{HEADER}{text}"))?;
        fs::rename(&temp, path)
    }

    /// The file read and checked; no file is no link
    pub fn load(path: &Path) -> Result<Self, LinksFileError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let path_buf = || path.to_path_buf();
        let content = fs::read_to_string(path).map_err(|source| LinksFileError::Io {
            path: path_buf(),
            source,
        })?;
        let config: LinksConfig =
            toml::from_str(&content).map_err(|source| LinksFileError::TomlParse {
                path: path_buf(),
                source,
            })?;
        config.check().map_err(|source| LinksFileError::Link {
            path: path_buf(),
            source,
        })?;
        Ok(config)
    }
}

/// The name a link is first offered: a device's file name (`ttyUSB0`), a TCP link's
/// `host:port`, a file's name without its extension
pub fn default_name(address: &Address) -> String {
    let file_name = |path: &Path, stem: bool| {
        let name = if stem {
            path.file_stem()
        } else {
            path.file_name()
        };
        name.map(|name| name.to_string_lossy().into_owned())
    };
    match address {
        Address::Serial(device) => file_name(Path::new(device), false),
        Address::Tcp(host_port) => Some(host_port.clone()),
        Address::Replay(path) => file_name(path, true),
    }
    .unwrap_or_else(|| address.to_string())
}

/// This machine's name, for `hosts`, read without starting any program
#[cfg(unix)]
pub fn this_host() -> Option<String> {
    let mut name = [0u8; 256];
    if unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len()) } != 0 {
        return None;
    }
    let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    Some(String::from_utf8_lossy(&name[..len]).into_owned()).filter(|name| !name.is_empty())
}

#[cfg(windows)]
pub fn this_host() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|name| !name.is_empty())
}

/// A link that doesn't pass the check, by its place in the file and its name
#[derive(Debug, PartialEq, Eq)]
pub struct SavedLinkError {
    pub index: usize,
    pub name: String,
    pub problem: String,
}

impl std::fmt::Display for SavedLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (number, name, problem) = (self.index + 1, &self.name, &self.problem);
        write!(f, "link #{number} '{name}': {problem}")
    }
}

impl std::error::Error for SavedLinkError {}

#[derive(Debug)]
pub enum LinksFileError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Link {
        path: PathBuf,
        source: SavedLinkError,
    },
}

impl std::fmt::Display for LinksFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read links at '{}': {source}", path.display())
            }
            Self::TomlParse { path, source } => {
                write!(f, "invalid syntax in '{}':\n{source}", path.display())
            }
            Self::Link { path, source } => {
                write!(f, "invalid link in '{}': {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LinksFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::TomlParse { source, .. } => Some(source),
            Self::Link { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
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
}
