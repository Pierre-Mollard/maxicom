use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::{App, types::FocusTarget},
    configs::{PrefixWrapBehavior, ThemeConfig},
    ui::common::{border_style_for, popup_area},
};

pub fn render_settings_popup(app: &App, frame: &mut Frame, theme: &ThemeConfig) {
    let area = popup_area(frame.area(), 40, 17, 42, 19);
    frame.render_widget(Clear, area);

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

    let text = vec![
        Line::from(Span::styled(
            "--- Configuration Dashboard ---",
            Style::default()
                .fg(theme.palette.ternary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [M] ", Style::default().fg(theme.palette.secondary)),
            Span::raw(" Input Mode  : "),
            Span::styled(mode_str, Style::default().fg(theme.palette.main)),
        ]),
        Line::from(vec![
            Span::styled(" [E] ", Style::default().fg(theme.palette.secondary)),
            Span::raw(" Local Echo  : "),
            Span::styled(echo_str, Style::default().fg(theme.palette.main)),
        ]),
        Line::from(vec![
            Span::styled(" [T] ", Style::default().fg(theme.palette.secondary)),
            Span::raw(" Timestamps  : "),
            Span::styled(timestamp_str, Style::default().fg(theme.palette.main)),
        ]),
        Line::from(vec![
            Span::styled(" [X] ", Style::default().fg(theme.palette.secondary)),
            Span::raw(" Rx Encoding : "),
            Span::styled(encoding_str, Style::default().fg(theme.palette.main)),
        ]),
        Line::from(vec![
            Span::styled(" [B] ", Style::default().fg(theme.palette.secondary)),
            Span::raw(" Byte Inspect: "),
            Span::styled(raw_display_str, Style::default().fg(theme.palette.main)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "--- Line Mode Specifics ---",
            Style::default()
                .fg(theme.palette.ternary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [W] ", Style::default().fg(theme.palette.secondary)),
            Span::raw(" Line Wrap   : "),
            Span::styled(wrap_str, Style::default().fg(theme.palette.main)),
        ]),
        Line::from(vec![
            Span::styled(" [I] ", Style::default().fg(theme.palette.secondary)),
            Span::raw(" Indent Wrap : "),
            Span::styled(indent_str, Style::default().fg(theme.palette.main)),
        ]),
        Line::from(vec![
            Span::styled(" [D] ", Style::default().fg(theme.palette.secondary)),
            Span::raw(" Display Mode : "),
            Span::styled(display_mode_str, Style::default().fg(theme.palette.main)),
        ]),
    ];

    let popup = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style_for(
                FocusTarget::Overlay,
                app.focus_target(),
                app.is_app_focused(),
                theme,
            ))
            .title(" Settings [Toggle Hotkeys] ")
            .style(Style::default().fg(theme.palette.ternary))
            .title_bottom(Line::from(vec![
                Span::styled("[Esc]: ", Style::default().fg(theme.palette.text_muted)),
                Span::raw("Return"),
            ])),
    );

    frame.render_widget(popup, area);
}
