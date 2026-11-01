use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    app::App,
    capture::shorten_home,
    configs::{AppConfig, PrefixWrapBehavior, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{PopupTarget, SettingKey, SettingsDialog},
    ui::popups::{FORM_WIDTH, form_area, key_hints, popup_block, row_area, section},
};

pub fn render_settings_popup(
    app: &App,
    dialog: &SettingsDialog,
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
    let dimension_str = app.settings().terminal_mode.dimension_mode.as_str();

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
    let answers_str = if app.settings().terminal_mode.answer_queries {
        "ON"
    } else {
        "OFF"
    };
    // each row with the setting a click on it toggles (MAX-113)
    let rows = vec![
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
        (None, Line::from("")),
        (None, section("Terminal Mode", theme)),
        (
            Some(SettingKey::DimensionMode),
            row("S", "Size Mode", dimension_str),
        ),
        (
            Some(SettingKey::AnswerQueries),
            row("A", "Query Answers", answers_str),
        ),
    ];
    let mut rows: Vec<(Option<PopupTarget>, Line)> = (rows.into_iter())
        .map(|(key, line)| (key.map(PopupTarget::Setting), line))
        .collect();
    // the config folder, then the files out of it; a click copies one (MAX-163)
    rows.push((None, Line::from("")));
    rows.push((None, section("Files", theme)));
    for (index, (what, path)) in AppConfig::config_paths(app.settings())
        .into_iter()
        .enumerate()
    {
        // "theme file" is `Theme`, the folder `[C] Config`
        let name = what.split(' ').next().unwrap_or(what);
        let mut chars = name.chars();
        let label: String = chars
            .next()
            .into_iter()
            .flat_map(char::to_uppercase)
            .chain(chars)
            .collect();
        // `c` copies the folder, a digit the file of that number (MAX-163)
        let key = if index == 0 {
            "[C] ".to_string()
        } else {
            format!("[{index}] ")
        };
        let line = Line::from(vec![
            Span::styled(key, Style::default().fg(theme.palette.secondary)),
            Span::raw(format!("{label:<13}: ")),
            Span::styled(shorten_home(&path), Style::default().fg(theme.palette.main)),
        ]);
        rows.push((Some(PopupTarget::ConfigPath(index)), line));
    }

    let hints = key_hints(
        &app.key_resolver,
        false,
        &[(&[KeyBindingDialogAction::Close], "Return")],
        theme,
    );
    let block = popup_block("Settings", &hints, theme);

    let area = form_area(frame.area(), FORM_WIDTH, rows.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // too short for every row: scrolled, a scrollbar in the right padding (MAX-163)
    let visible = inner.height as usize;
    let max_scroll = rows.len().saturating_sub(visible);
    dialog.max_scroll.set(max_scroll);
    let scroll = dialog.scroll.min(max_scroll);
    for (y, (target, _)) in rows.iter().enumerate().skip(scroll).take(visible) {
        if let Some(target) = target {
            targets.push((row_area(inner, y - scroll), *target));
        }
    }
    hints.push_block_targets(area, targets);
    let text: Vec<Line> = rows.into_iter().map(|(_, line)| line).collect();
    frame.render_widget(Paragraph::new(text).scroll((scroll as u16, 0)), inner);
    if max_scroll > 0 {
        let mut state = ScrollbarState::new(max_scroll)
            .viewport_content_length(visible)
            .position(scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .thumb_symbol("█")
            .track_symbol(Some("│"))
            .thumb_style(*theme.ui.scrollbar_thumb)
            .track_style(*theme.ui.scrollbar_track)
            .begin_style(*theme.ui.scrollbar_thumb)
            .end_style(*theme.ui.scrollbar_thumb);
        let bar = Rect {
            x: inner.right(),
            width: 1,
            ..inner
        }
        .intersection(area);
        frame.render_stateful_widget(scrollbar, bar, &mut state);
    }
    area
}
