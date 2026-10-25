use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph, Wrap},
};

use crate::{
    configs::{KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::ErrorDialog,
    ui::{
        common::popup_area,
        popups::{FORM_WIDTH, key_hints},
    },
};

pub fn render_disconnected_popup(
    port: &str,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
) {
    let text = vec![
        Line::from(Span::styled(
            "⚠ SERIAL DEVICE DISCONNECTED ⚠",
            Style::default()
                .fg(theme.palette.text_warn)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("Device "),
            Span::styled(port, Style::default().fg(theme.palette.secondary)),
            Span::raw(" was closed or unplugged."),
        ]),
        Line::from("Waiting for port to reconnect..."),
        Line::from(""),
        key_hints(
            resolver,
            false,
            &[(
                &[
                    KeyBindingDialogAction::Close,
                    KeyBindingDialogAction::Confirm,
                ],
                "Quit",
            )],
            theme,
        ),
    ];

    render_alert("Connection Lost", text, frame, theme);
}

pub fn render_detached_popup(
    port: &str,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
) {
    let text = vec![
        Line::from(Span::styled(
            "⚠ SERIAL DEVICE DETACHED ⚠",
            Style::default()
                .fg(theme.palette.text_warn)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("Device "),
            Span::styled(port, Style::default().fg(theme.palette.secondary)),
            Span::raw(" was released."),
        ]),
        Line::from("The serial port is free..."),
        Line::from(""),
        key_hints(
            resolver,
            false,
            &[
                (&[KeyBindingDialogAction::Confirm], "Reattach"),
                (&[KeyBindingDialogAction::Close], "Quit"),
            ],
            theme,
        ),
    ];

    render_alert("Connection Lost", text, frame, theme);
}

pub fn render_error_popup(
    err: &ErrorDialog,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
) {
    let mut lines = vec![
        Line::from(Span::styled(
            format!("⚠  {}  ⚠", err.title),
            Style::default()
                .fg(theme.palette.text_warn)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            &err.summary,
            Style::default().fg(theme.palette.text),
        )),
    ];

    if let Some(ref details) = err.details {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Cause: ", Style::default().fg(theme.palette.text_muted)),
            Span::styled(details, Style::default().fg(theme.palette.secondary)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(key_hints(
        resolver,
        false,
        &[(
            &[
                KeyBindingDialogAction::Close,
                KeyBindingDialogAction::Confirm,
            ],
            "Dismiss",
        )],
        theme,
    ));

    render_alert(err.title, lines, frame, theme);
}

/// A warning box sized to `lines` once wrapped at its width (a long port path or error cause
/// wraps)
fn render_alert(title: &str, lines: Vec<Line>, frame: &mut Frame, theme: &ThemeConfig) {
    let warn = Style::default().fg(theme.palette.text_warn);
    let block = Block::bordered()
        .title(Span::styled(
            format!(" {title} "),
            warn.add_modifier(Modifier::BOLD),
        ))
        .border_style(warn)
        .style(*theme.popup.background)
        .padding(Padding::horizontal(1));

    // border and padding on each side
    let inner_width = FORM_WIDTH.min(frame.area().width).saturating_sub(4).max(1) as usize;
    let rows: usize = lines
        .iter()
        .map(|line| line.width().div_ceil(inner_width).max(1))
        .sum();
    let area = popup_area(frame.area(), FORM_WIDTH, rows as u16 + 2);
    frame.render_widget(Clear, area);

    let inner = block.inner(area);
    frame.render_widget(block, area);
    let popup = Paragraph::new(lines)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(popup, inner);
}
