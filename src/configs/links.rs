use std::{
    fs, io,
    path::{Path, PathBuf},
};

use ratatui::style::Color;
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

/// A link as written: its name, the machines it's for, its address, its color in place of the
/// palette's (MAX-201), and the line settings that differ from `[link]`'s (none on a TCP link)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedLink {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
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
#[path = "../tests/configs/links.rs"]
mod tests;
