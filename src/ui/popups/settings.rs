use ratatui::{
    Frame,
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::App,
    configs::{PrefixWrapBehavior, ThemeConfig, keybindings::KeyBindingDialogAction},
    ui::popups::{FORM_WIDTH, form_area, key_hints, popup_block, section},
};

pub fn render_settings_popup(app: &App, frame: &mut Frame, theme: &ThemeConfig) {
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

    let text = vec![
        section("General", theme),
        row("M", "Input Mode", mode_str),
        row("E", "Local Echo", echo_str),
        row("T", "Timestamps", timestamp_str),
        row("X", "Rx Encoding", encoding_str),
        row("B", "Byte Inspect", raw_display_str),
        Line::from(""),
        section("Line Mode", theme),
        row("W", "Line Wrap", wrap_str),
        row("I", "Indent Wrap", indent_str),
        row("D", "Display Mode", display_mode_str),
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

    let area = form_area(frame.area(), FORM_WIDTH, text.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(text), inner);
}
