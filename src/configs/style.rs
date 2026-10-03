use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(into = "ConfigStyleDef", from = "ConfigStyleDef")]
pub struct ConfigStyle {
    style: Style,
}

impl ConfigStyle {
    pub const fn new(style: Style) -> Self {
        Self { style }
    }

    pub const fn from_fg(fg: Color) -> Self {
        Self {
            style: Style::new().fg(fg),
        }
    }

    pub const fn from_bg(bg: Color) -> Self {
        Self {
            style: Style::new().bg(bg),
        }
    }

    pub const fn to_style(self) -> Style {
        self.style
    }
}

impl std::ops::Deref for ConfigStyle {
    type Target = Style;

    fn deref(&self) -> &Self::Target {
        &self.style
    }
}

impl From<ConfigStyle> for Style {
    fn from(s: ConfigStyle) -> Self {
        s.style
    }
}

impl From<Style> for ConfigStyle {
    fn from(style: Style) -> Self {
        Self { style }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum ConfigStyleDef {
    Simple(Color),
    Detailed {
        #[serde(skip_serializing_if = "Option::is_none")]
        fg: Option<Color>,
        #[serde(skip_serializing_if = "Option::is_none")]
        bg: Option<Color>,
        #[serde(skip_serializing_if = "Option::is_none")]
        underline_color: Option<Color>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        bold: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        dim: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        italic: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        underlined: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        slow_blink: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rapid_blink: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reversed: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        hidden: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        crossed: bool,
    },
}

impl From<ConfigStyleDef> for ConfigStyle {
    fn from(def: ConfigStyleDef) -> Self {
        match def {
            ConfigStyleDef::Simple(color) => ConfigStyle::from_fg(color),
            ConfigStyleDef::Detailed {
                fg,
                bg,
                underline_color,
                bold,
                dim,
                italic,
                underlined,
                slow_blink,
                rapid_blink,
                reversed,
                hidden,
                crossed,
            } => {
                let mut s = Style::default();
                if let Some(c) = fg {
                    s = s.fg(c);
                }
                if let Some(c) = bg {
                    s = s.bg(c);
                }
                if let Some(uc) = underline_color {
                    s = s.underline_color(uc);
                }
                if bold {
                    s = s.add_modifier(Modifier::BOLD);
                }
                if dim {
                    s = s.add_modifier(Modifier::DIM);
                }
                if italic {
                    s = s.add_modifier(Modifier::ITALIC);
                }
                if underlined {
                    s = s.add_modifier(Modifier::UNDERLINED);
                }
                if slow_blink {
                    s = s.add_modifier(Modifier::SLOW_BLINK);
                }
                if rapid_blink {
                    s = s.add_modifier(Modifier::RAPID_BLINK);
                }
                if reversed {
                    s = s.add_modifier(Modifier::REVERSED);
                }
                if hidden {
                    s = s.add_modifier(Modifier::HIDDEN);
                }
                if crossed {
                    s = s.add_modifier(Modifier::CROSSED_OUT);
                }
                ConfigStyle { style: s }
            }
        }
    }
}

impl From<ConfigStyle> for ConfigStyleDef {
    fn from(t: ConfigStyle) -> Self {
        let style = t.style;
        let mods = style.add_modifier;

        if style.bg.is_none()
            && style.underline_color.is_none()
            && mods.is_empty()
            && let Some(fg) = style.fg
        {
            return ConfigStyleDef::Simple(fg);
        }

        ConfigStyleDef::Detailed {
            fg: style.fg,
            bg: style.bg,
            underline_color: style.underline_color,
            bold: mods.contains(Modifier::BOLD),
            dim: mods.contains(Modifier::DIM),
            italic: mods.contains(Modifier::ITALIC),
            underlined: mods.contains(Modifier::UNDERLINED),
            slow_blink: mods.contains(Modifier::SLOW_BLINK),
            rapid_blink: mods.contains(Modifier::RAPID_BLINK),
            reversed: mods.contains(Modifier::REVERSED),
            hidden: mods.contains(Modifier::HIDDEN),
            crossed: mods.contains(Modifier::CROSSED_OUT),
        }
    }
}
