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
    link::{Address, LinkKind},
    overlays::{ErrorDialog, PopupTarget},
    ui::{
        common::popup_area,
        popups::{FORM_WIDTH, Hints, key_hints},
        status_bar::write_human,
    },
};

/// What the disconnected and detached popups say for each kind of link (MAX-168): the
/// headline, the address between the words before and after it, the next line
struct LinkAlert {
    headline: &'static str,
    before: &'static str,
    after: &'static str,
    next: &'static str,
}

impl LinkAlert {
    fn disconnected(kind: LinkKind) -> Self {
        match kind {
            LinkKind::Serial => Self {
                headline: "⚠ SERIAL PORT DISCONNECTED ⚠",
                before: "Port ",
                after: " was closed or unplugged.",
                next: "Waiting for it to come back...",
            },
            LinkKind::Tcp => Self {
                headline: "⚠ TCP CONNECTION LOST ⚠",
                before: "",
                after: " closed the connection or stopped answering.",
                next: "Reconnecting...",
            },
            // a replay never reopens, so this popup isn't shown for one
            LinkKind::Replay => Self {
                headline: "⚠ REPLAY STOPPED ⚠",
                before: "File ",
                after: " can't be read.",
                next: "",
            },
        }
    }

    fn detached(kind: LinkKind) -> Self {
        match kind {
            LinkKind::Serial => Self {
                headline: "⚠ SERIAL PORT DETACHED ⚠",
                before: "Port ",
                after: " was released.",
                next: "Other programs can open it now.",
            },
            LinkKind::Tcp => Self {
                headline: "⚠ TCP CONNECTION DETACHED ⚠",
                before: "The connection to ",
                after: " was closed.",
                next: "Another client can connect now.",
            },
            // a replay doesn't detach, so this popup isn't shown for one
            LinkKind::Replay => Self {
                headline: "⚠ REPLAY DETACHED ⚠",
                before: "File ",
                after: " was released.",
                next: "",
            },
        }
    }

    fn lines(self, address: &Address, theme: &ThemeConfig) -> Vec<Line<'static>> {
        vec![
            Line::from(Span::styled(
                self.headline,
                Style::default()
                    .fg(theme.palette.text_warn)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::raw(self.before),
                Span::styled(
                    address.to_string(),
                    Style::default().fg(theme.palette.secondary),
                ),
                Span::raw(self.after),
            ]),
            Line::from(self.next),
            Line::from(""),
        ]
    }
}

pub fn render_disconnected_popup(
    address: &Address,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let text = LinkAlert::disconnected(address.kind()).lines(address, theme);
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
    let text = LinkAlert::detached(address.kind()).lines(address, theme);
    let hints = key_hints(
        resolver,
        false,
        &[
            (&[KeyBindingDialogAction::Confirm], "Reattach"),
            (&[KeyBindingDialogAction::Close], "Quit"),
        ],
        theme,
    );

    render_alert("Detached", text, hints, frame, theme, targets)
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
