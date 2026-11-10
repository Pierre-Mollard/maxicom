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
    Detailed(StyleTable),
}

/// A style written as a table: its colors and the modifiers set. Other tables put these keys
/// beside their own, like a highlight group's (MAX-216)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleTable {
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
}

impl From<StyleTable> for ConfigStyle {
    fn from(table: StyleTable) -> Self {
        let mut s = Style::default();
        if let Some(c) = table.fg {
            s = s.fg(c);
        }
        if let Some(c) = table.bg {
            s = s.bg(c);
        }
        if let Some(uc) = table.underline_color {
            s = s.underline_color(uc);
        }
        let modifiers = [
            (table.bold, Modifier::BOLD),
            (table.dim, Modifier::DIM),
            (table.italic, Modifier::ITALIC),
            (table.underlined, Modifier::UNDERLINED),
            (table.slow_blink, Modifier::SLOW_BLINK),
            (table.rapid_blink, Modifier::RAPID_BLINK),
            (table.reversed, Modifier::REVERSED),
            (table.hidden, Modifier::HIDDEN),
            (table.crossed, Modifier::CROSSED_OUT),
        ];
        for (set, modifier) in modifiers {
            if set {
                s = s.add_modifier(modifier);
            }
        }
        ConfigStyle { style: s }
    }
}

impl From<ConfigStyle> for StyleTable {
    fn from(t: ConfigStyle) -> Self {
        let style = t.style;
        let mods = style.add_modifier;
        StyleTable {
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

impl From<ConfigStyleDef> for ConfigStyle {
    fn from(def: ConfigStyleDef) -> Self {
        match def {
            ConfigStyleDef::Simple(color) => ConfigStyle::from_fg(color),
            ConfigStyleDef::Detailed(table) => table.into(),
        }
    }
}

impl ConfigStyle {
    /// Only a text color: written as that color alone
    pub fn is_fg_only(self) -> bool {
        let style = self.style;
        style.fg.is_some()
            && style.bg.is_none()
            && style.underline_color.is_none()
            && style.add_modifier.is_empty()
    }
}

impl From<ConfigStyle> for ConfigStyleDef {
    fn from(t: ConfigStyle) -> Self {
        match t.style.fg {
            Some(fg) if t.is_fg_only() => ConfigStyleDef::Simple(fg),
            _ => ConfigStyleDef::Detailed(t.into()),
        }
    }
}
