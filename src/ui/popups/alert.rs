use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph, Wrap},
};

use crate::{
    capture::Session,
    configs::{KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    link::Address,
    overlays::{ErrorDialog, PopupTarget},
    ui::{
        common::popup_area,
        popups::{FORM_WIDTH, Hints, key_hints},
        status_bar::write_human,
    },
};

pub fn render_disconnected_popup(
    address: &Address,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
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
            Span::styled(
                address.to_string(),
                Style::default().fg(theme.palette.secondary),
            ),
            Span::raw(" was closed or unplugged."),
        ]),
        Line::from("Waiting for port to reconnect..."),
        Line::from(""),
    ];
    let hints = key_hints(
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
    );

    render_alert("Connection Lost", text, hints, frame, theme, targets)
}

pub fn render_detached_popup(
    address: &Address,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
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
            Span::styled(
                address.to_string(),
                Style::default().fg(theme.palette.secondary),
            ),
            Span::raw(" was released."),
        ]),
        Line::from("The serial port is free..."),
        Line::from(""),
    ];
    let hints = key_hints(
        resolver,
        false,
        &[
            (&[KeyBindingDialogAction::Confirm], "Reattach"),
            (&[KeyBindingDialogAction::Close], "Quit"),
        ],
        theme,
    );

    render_alert("Connection Lost", text, hints, frame, theme, targets)
}

pub fn render_error_popup(
    err: &ErrorDialog,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
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
    let hints = key_hints(
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
    );

    render_alert(err.title, lines, hints, frame, theme, targets)
}

pub fn render_confirm_quit_popup(
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let text = vec![
        Line::from(Span::styled(
            "Quit maxicom?",
            Style::default()
                .fg(theme.palette.text)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    let hints = key_hints(
        resolver,
        false,
        &[
            (&[KeyBindingDialogAction::Confirm], "Quit"),
            (&[KeyBindingDialogAction::Close], "Cancel"),
        ],
        theme,
    );

    render_alert("Quit", text, hints, frame, theme, targets)
}

/// The startup question when a past session on this device can be restored (MAX-148)
pub fn render_restore_popup(
    session: &Session,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let mut size = String::new();
    write_human(&mut size, session.raw_len);
    let text = vec![
        Line::from(Span::styled(
            format!("Restore the last session on {}?", session.port_tag()),
            Style::default()
                .fg(theme.palette.text)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("{}, {size}B received", session.time_span()),
            Style::default().fg(theme.palette.text_muted),
        )),
        Line::from(""),
    ];
    let hints = key_hints(
        resolver,
        false,
        &[
            (&[KeyBindingDialogAction::Confirm], "Restore"),
            (&[KeyBindingDialogAction::Close], "Skip"),
        ],
        theme,
    );

    render_alert("Restore", text, hints, frame, theme, targets)
}

/// A warning box sized to `lines` and the `hints` under them once wrapped at its width (a long
/// port path or error cause wraps). The hints are click targets (MAX-125)
fn render_alert<'a>(
    title: &str,
    mut lines: Vec<Line<'a>>,
    hints: Hints<'a>,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let warn = Style::default().fg(theme.palette.text_warn);
    let block = Block::bordered()
        .title(Span::styled(
            format!(" {title} "),
            warn.add_modifier(Modifier::BOLD),
        ))
        .border_style(warn)
        .style(*theme.popup.background)
        .padding(Padding::horizontal(1));

    let hints_width = hints.line.width() as u16;
    lines.push(hints.line.clone());

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
    // the last row, centered as ratatui's paragraph centers it
    let hints_x = inner.x + (inner.width / 2).saturating_sub(hints_width / 2);
    hints.push_targets((hints_x, inner.bottom().saturating_sub(1)), inner, targets);
    let popup = Paragraph::new(lines)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(popup, inner);
    area
}
