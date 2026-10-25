use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::App,
    configs::{PrefixWrapBehavior, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{PopupTarget, SettingKey},
    ui::popups::{FORM_WIDTH, form_area, key_hints, popup_block, row_area, section},
};

pub fn render_settings_popup(
    app: &App,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let mode_str = match app.settings().mode {
        crate::configs::Mode::Line => "Line Mode",
        crate::configs::Mode::Terminal => "Terminal Mode",
    };
    let wrap_str = match app.settings().line_mode.wrap_mode {
        crate::configs::WrapMode::Word => "Word-Wrap",
        crate::configs::WrapMode::Char => "Char-Wrap",
        crate::configs::WrapMode::Truncate => "Truncate",
    };
    let indent_str = match app.settings().line_mode.wrap_indent {
        PrefixWrapBehavior::None => "None",
        PrefixWrapBehavior::Prefix => "Prefix",
        PrefixWrapBehavior::Timestamp => "Timestamp",
    };
    let echo_str = if app.settings().local_echo {
        "ON"
    } else {
        "OFF"
    };
    let timestamp_str = if app.settings().show_timestamps {
        "ON"
    } else {
        "OFF"
    };
    let encoding_str = app.settings().encoding.as_str();
    let display_mode_str = app.settings().line_mode.display_mode.as_str();
    let raw_display_str = app.settings().raw_side.view.as_str();

    let row = |key: &'static str, label: &'static str, value: &'static str| {
        Line::from(vec![
            Span::styled(
                format!("[{key}] "),
                Style::default().fg(theme.palette.secondary),
            ),
            Span::raw(format!("{label:<13}: ")),
            Span::styled(value, Style::default().fg(theme.palette.main)),
        ])
    };

    let mouse_str = if app.settings().ui.mouse { "ON" } else { "OFF" };
    // each row with the setting a click on it toggles (MAX-113)
    let rows = [
        (None, section("General", theme)),
        (Some(SettingKey::Mode), row("M", "Input Mode", mode_str)),
        (
            Some(SettingKey::LocalEcho),
            row("E", "Local Echo", echo_str),
        ),
        (
            Some(SettingKey::DisplayTimestamp),
            row("T", "Timestamps", timestamp_str),
        ),
        (
            Some(SettingKey::TextEncoding),
            row("X", "Rx Encoding", encoding_str),
        ),
        (
            Some(SettingKey::DisplayRaw),
            row("B", "Byte Inspect", raw_display_str),
        ),
        (Some(SettingKey::Mouse), row("U", "Mouse", mouse_str)),
        (None, Line::from("")),
        (None, section("Line Mode", theme)),
        (Some(SettingKey::LineWrap), row("W", "Line Wrap", wrap_str)),
        (
            Some(SettingKey::IdentWrap),
            row("I", "Indent Wrap", indent_str),
        ),
        (
            Some(SettingKey::DisplayMode),
            row("D", "Display Mode", display_mode_str),
        ),
    ];

    let block = popup_block(
        "Settings",
        key_hints(
            &app.key_resolver,
            false,
            &[(&[KeyBindingDialogAction::Close], "Return")],
            theme,
        ),
        theme,
    );

    let area = form_area(frame.area(), FORM_WIDTH, rows.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    for (y, (key, _)) in rows.iter().enumerate() {
        if let Some(key) = key {
            targets.push((row_area(inner, y), PopupTarget::Setting(*key)));
        }
    }
    let text: Vec<Line> = rows.into_iter().map(|(_, line)| line).collect();
    frame.render_widget(Paragraph::new(text), inner);
    area
}
