use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph, Wrap},
};

use crate::{
    app::{capture::GroupLoad, jobs::Ask},
    capture::Session,
    commands::{Access, Origin, value::pictures},
    configs::{
        KeyPattern, KeyResolver, Mode, ThemeConfig, keybindings::KeyBindingDialogAction,
        links::SavedLink,
    },
    link::LinkKind,
    overlays::{ErrorDialog, PopupTarget, sessions::sessions as sessions_count},
    ui::{
        common::popup_area,
        popups::{FORM_WIDTH, Hints, hint_spans, key_hints},
        status_bar::write_human,
        transfer_row::{bytes_text, clock, gauge, rate_text},
    },
};

/// The loading popup's spinner, a step every `SPINNER_STEP`, and its gauge's width (MAX-272)
pub const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
pub const SPINNER_STEP: Duration = Duration::from_millis(100);
const LOADING_GAUGE: usize = 30;

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
                headline: "◆ SERIAL PORT DISCONNECTED ◆",
                before: "Port ",
                after: " was closed or unplugged.",
                next: "Waiting for it to come back...",
            },
            LinkKind::Tcp(_) => Self {
                headline: "◆ TCP CONNECTION LOST ◆",
                before: "",
                after: " closed the connection or stopped answering.",
                next: "Reconnecting...",
            },
            // a replay never reopens, so this popup isn't shown for one
            LinkKind::Replay => Self {
                headline: "◆ REPLAY STOPPED ◆",
                before: "File ",
                after: " can't be read.",
                next: "",
            },
        }
    }

    fn detached(kind: LinkKind) -> Self {
        match kind {
            LinkKind::Serial => Self {
                headline: "◆ SERIAL PORT DETACHED ◆",
                before: "Port ",
                after: " was released.",
                next: "Other programs can open it now.",
            },
            LinkKind::Tcp(_) => Self {
                headline: "◆ TCP CONNECTION DETACHED ◆",
                before: "The connection to ",
                after: " was closed.",
                next: "Another client can connect now.",
            },
            // a replay doesn't detach, so this popup isn't shown for one
            LinkKind::Replay => Self {
                headline: "◆ REPLAY DETACHED ◆",
                before: "File ",
                after: " was released.",
                next: "",
            },
        }
    }

    fn lines(self, label: String, theme: &ThemeConfig) -> Vec<Line<'static>> {
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
                Span::styled(label, Style::default().fg(theme.palette.secondary)),
                Span::raw(self.after),
            ]),
            Line::from(self.next),
            Line::from(""),
        ]
    }
}

/// The link's popups name it as the notices do: `label`, a saved one by its name (MAX-185).
/// The read error that closed it, if any, under the next line (MAX-199)
pub fn render_disconnected_popup(
    kind: LinkKind,
    label: String,
    reason: Option<&str>,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let mut text = LinkAlert::disconnected(kind).lines(label, theme);
    if let Some(reason) = reason {
        let reason = Span::styled(
            reason.to_string(),
            Style::default().fg(theme.palette.text_muted),
        );
        text.insert(text.len() - 1, Line::from(reason));
    }
    let hints = link_hints(resolver, None, theme);

    let tone = theme.palette.text_warn;
    render_alert("Connection Lost", tone, text, hints, frame, theme, targets)
}

pub fn render_detached_popup(
    kind: LinkKind,
    label: String,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let text = LinkAlert::detached(kind).lines(label, theme);
    let hints = link_hints(resolver, Some("Reattach"), theme);

    let tone = theme.palette.text_warn;
    render_alert("Detached", tone, text, hints, frame, theme, targets)
}

/// The link popups' keys: Enter for `confirm` (Quit when none), `d` the directory to pick
/// another device (MAX-181), Esc to quit
fn link_hints<'a>(
    resolver: &KeyResolver,
    confirm: Option<&'a str>,
    theme: &ThemeConfig,
) -> Hints<'a> {
    let key = |action| {
        let key = resolver.dialog_keys(action).into_iter().next();
        key.map(|key| (key, PopupTarget::Hint(action)))
            .into_iter()
            .collect::<Vec<_>>()
    };
    let directory = KeyPattern {
        code: KeyCode::Char('d'),
        modifiers: KeyModifiers::NONE,
    };
    let directory = vec![(directory, PopupTarget::OpenDirectory)];
    let hints = match confirm {
        Some(label) => vec![
            (key(KeyBindingDialogAction::Confirm), label),
            (directory, "Directory"),
            (key(KeyBindingDialogAction::Close), "Quit"),
        ],
        None => {
            let mut quit = key(KeyBindingDialogAction::Close);
            quit.extend(key(KeyBindingDialogAction::Confirm));
            vec![(directory, "Directory"), (quit, "Quit")]
        }
    };
    let (line, spots) = hint_spans(hints, theme);
    Hints { line, spots }
}

/// Started without a device, the directory closed before one was picked (MAX-181)
pub fn render_no_device_popup(
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let text = vec![
        Line::from(Span::styled(
            "◆ NO DEVICE ◆",
            Style::default()
                .fg(theme.palette.text_warn)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("No device is open."),
        Line::from("Pick one in the directory."),
        Line::from(""),
    ];
    let hints = key_hints(
        resolver,
        false,
        &[
            (&[KeyBindingDialogAction::Confirm], "Directory"),
            (&[KeyBindingDialogAction::Close], "Quit"),
        ],
        theme,
    );
    let tone = theme.palette.text_warn;
    render_alert("No Device", tone, text, hints, frame, theme, targets)
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
            format!("◆  {}  ◆", err.title),
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
            "Close",
        )],
        theme,
    );

    let tone = theme.palette.text_warn;
    render_alert(err.title, tone, lines, hints, frame, theme, targets)
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

    let tone = theme.palette.main;
    render_alert("Quit", tone, text, hints, frame, theme, targets)
}

/// The spinner's glyph `elapsed` after it started (MAX-272)
pub fn spinner(elapsed: Duration) -> char {
    let turn = elapsed.as_millis() / SPINNER_STEP.as_millis();
    SPINNER[turn as usize % SPINNER.len()]
}

/// A group replay loading (MAX-272): a spinner with the mode and the session being read, the
/// gauge, the bytes, the rate and the time left; the close key stops it
pub fn render_loading_popup(
    load: &GroupLoad,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let now = Instant::now();
    let text = Style::default().fg(theme.palette.text);
    let muted = Style::default().fg(theme.palette.text_muted);
    let accent = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);

    let spinner = spinner(load.elapsed(now));
    let (mode, session, of) = load.at();
    let mode = match mode {
        Mode::Line => "Line mode",
        Mode::Terminal => "Terminal mode",
    };
    let (done, total) = load.progress();
    let percent = load.percent();
    let mut gauge_line = gauge(percent, LOADING_GAUGE, accent, muted).to_vec();
    gauge_line.push(Span::styled(format!(" {percent:>3}%"), text));
    let mut numbers = vec![Span::styled(bytes_text(done, Some(total)), muted)];
    if let Some(rate) = load.rate(now) {
        numbers.push(Span::styled(format!("  {}", rate_text(rate)), muted));
    }
    if let Some(eta) = load.eta(now) {
        numbers.push(Span::styled(format!("  ETA {}", clock(eta)), muted));
    }
    let lines = vec![
        Line::from(vec![
            Span::styled(format!("{spinner} "), accent),
            Span::styled(format!("{mode}, session {session} of {of}"), text),
        ]),
        Line::from(""),
        Line::from(gauge_line),
        Line::from(numbers),
        Line::from(""),
    ];
    let hints = key_hints(
        resolver,
        false,
        &[(&[KeyBindingDialogAction::Close], "Stop")],
        theme,
    );
    let title = format!("Loading {}", sessions_count(load.sessions()));
    render_alert(
        &title,
        theme.palette.main,
        lines,
        hints,
        frame,
        theme,
        targets,
    )
}

/// Whether to delete a saved link (MAX-185): its name and address, yes or no
pub fn render_delete_link_popup(
    link: &SavedLink,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let text = vec![
        Line::from(Span::styled(
            format!("Delete the link '{}'?", link.name),
            Style::default()
                .fg(theme.palette.text)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            link.address.clone(),
            Style::default().fg(theme.palette.text_muted),
        )),
        Line::from(""),
    ];
    let hints = key_hints(
        resolver,
        false,
        &[
            (&[KeyBindingDialogAction::Confirm], "Delete"),
            (&[KeyBindingDialogAction::Close], "Cancel"),
        ],
        theme,
    );
    let tone = theme.palette.text_warn;
    render_alert("Delete link", tone, text, hints, frame, theme, targets)
}

/// A client's command the `[api]` levels ask about (MAX-287): who asks, the command as typed
/// (control characters as their pictures), what it reaches and the seconds left to answer;
/// send, always for this client, refuse
pub fn render_ask_popup(
    ask: &Ask,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let client = match &ask.client {
        Origin::Client {
            name: Some(name), ..
        } => format!("'{name}'"),
        Origin::Client { conn, .. } => format!("client {conn}"),
        origin => origin.kind().to_string(),
    };
    let (reaches, tone) = match ask.access {
        Access::Device => ("It reaches the device", theme.palette.text_warn),
        Access::Control | Access::Read => ("It changes maxicom", theme.palette.main),
    };
    let left = ask
        .until
        .saturating_duration_since(Instant::now())
        .as_secs();
    let text = vec![
        Line::from(Span::styled(
            format!("The API client {client} wants to run"),
            Style::default()
                .fg(theme.palette.text)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            pictures(&ask.command),
            Style::default().fg(theme.palette.secondary),
        )),
        Line::from(Span::styled(
            format!("{reaches}: refused in {left} s"),
            Style::default().fg(theme.palette.text_muted),
        )),
        Line::from(""),
    ];
    let dialog_key = |action| resolver.dialog_keys(action).into_iter().next();
    let always = KeyPattern {
        code: KeyCode::Char('a'),
        modifiers: KeyModifiers::NONE,
    };
    let buttons = [
        (
            dialog_key(KeyBindingDialogAction::Confirm),
            PopupTarget::Hint(KeyBindingDialogAction::Confirm),
            "Send",
        ),
        (Some(always), PopupTarget::AskAlways, "Always"),
        (
            dialog_key(KeyBindingDialogAction::Close),
            PopupTarget::Hint(KeyBindingDialogAction::Close),
            "Refuse",
        ),
    ];
    let buttons = buttons
        .into_iter()
        .map(|(key, target, label)| (key.map(|key| (key, target)).into_iter().collect(), label));
    let (line, spots) = hint_spans(buttons, theme);
    let hints = Hints { line, spots };
    render_alert("API", tone, text, hints, frame, theme, targets)
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

    let tone = theme.palette.text_good;
    render_alert("Restore", tone, text, hints, frame, theme, targets)
}

/// A box sized to `lines` and the `hints` under them once wrapped at its width (a long port
/// path or error cause wraps), its border and title in `tone`: warn for a problem, else the
/// question's (MAX-163). The hints are click targets (MAX-125)
fn render_alert<'a>(
    title: &str,
    tone: Color,
    mut lines: Vec<Line<'a>>,
    hints: Hints<'a>,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let tone = Style::default().fg(tone);
    let block = Block::bordered()
        .title(Span::styled(
            format!(" {title} "),
            tone.add_modifier(Modifier::BOLD),
        ))
        .border_style(tone)
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
