/// Application configuration and files

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EolMode {
    CrLf,
    Cr,
    Lf,
}

impl EolMode {
    pub fn as_bytes(&self) -> &'static [u8] {
        match self {
            EolMode::CrLf => b"\r\n",
            EolMode::Cr => b"\r",
            EolMode::Lf => b"\n",
        }
    }
}

/// MaxiCom UI & session settings
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub local_echo: bool,
    pub eol_mode: EolMode,
    pub max_history_lines: usize,
    pub timestamp_lines: bool,
    pub display_partial_rx: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            local_echo: false,
            eol_mode: EolMode::CrLf,
            max_history_lines: 1000,
            timestamp_lines: false,
            display_partial_rx: true,
        }
    }
}
