use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::{
        App,
        types::{ClickAction, ClickTarget},
    },
    configs::{KeyBindingAction, ThemeConfig},
    transfer::FileTransfer,
    ui::status_bar::write_human,
};

/// The gauge's width between its brackets: as wide as the row allows, within these
const GAUGE_MIN: usize = 8;
const GAUGE_MAX: usize = 30;
/// At the row's right end: a click stops the transfer
const CANCEL: &str = " ✕ ";

/// The file being sent and its progress, on a row of its own (MAX-162):
///
/// ` ⇡ fw.bin YMODEM [██████░░░░]  54%  70K/131K  11K/s  ETA 0:05  2 retries        ✕ `
///
/// A download has `⇣`, and without a size (XMODEM) only the bytes received. A protocol
/// transfer waiting for the other side says so instead of the numbers. On a narrow
/// row the parts go, the retries first, then the time left, the rate, the bytes and the
/// protocol, the gauge last. Returns the cancel button as a click target
pub fn render_transfer_row(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &ThemeConfig,
) -> Vec<ClickTarget> {
    let Some(transfer) = app.transfer() else {
        return Vec::new();
    };
    let spans = row_spans(transfer, Instant::now(), area.width as usize, theme);
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line).style(*theme.ui.background), area);

    let cancel_width = CANCEL.chars().count() as u16;
    let cell = Rect::new(
        area.right().saturating_sub(cancel_width),
        area.y,
        cancel_width.min(area.width),
        1,
    );
    vec![ClickTarget {
        area: cell,
        action: ClickAction::Run(KeyBindingAction::CancelCommands),
        hover: Some(*theme.ui.hover),
    }]
}

/// The row's parts fitted to `width`, the cancel button right-aligned
fn row_spans(
    transfer: &FileTransfer,
    now: Instant,
    width: usize,
    theme: &ThemeConfig,
) -> Vec<Span<'static>> {
    let muted = Style::default().fg(theme.palette.text_muted);
    let accent = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);
    let text = Style::default().fg(theme.palette.text);
    let warn = Style::default().fg(theme.palette.text_warn);

    // ⇡ to the device, ⇣ from it (`download`)
    let arrow = if transfer.is_download() { '⇣' } else { '⇡' };
    let mut head = vec![Span::styled(
        format!(" {arrow} {} ", transfer.name()),
        accent,
    )];
    let protocol = transfer
        .protocol()
        .map(|protocol| Span::styled(format!("{} ", protocol.name()), muted));

    let mut spans;
    if transfer.waiting() {
        head.extend(protocol);
        let wait = if transfer.is_download() {
            "waiting for the sender (sb, sx) "
        } else {
            "waiting for the receiver (C or NAK) "
        };
        head.push(Span::styled(wait, muted));
        spans = head;
    } else {
        // an XMODEM download has no size: the bytes only, no gauge or percent
        let size = transfer.size();
        let percent = match size {
            Some(_) => Span::styled(format!(" {:>3}% ", transfer.percent()), text),
            None => Span::raw(""),
        };
        let mut bytes = String::new();
        write_human(&mut bytes, transfer.sent());
        if let Some(size) = size {
            bytes.push('/');
            write_human(&mut bytes, size);
        }
        let rate = transfer.rate(now).map(|rate| {
            let mut out = String::new();
            write_human(&mut out, rate as u64);
            Span::styled(format!(" {out}/s "), muted)
        });
        let eta =
            (transfer.eta(now)).map(|eta| Span::styled(format!(" ETA {} ", clock(eta)), muted));
        let retries = (transfer.retries())
            .filter(|&retries| retries > 0)
            .map(|retries| Span::styled(format!(" {retries} retries "), warn));
        // in the order they go when the row is too narrow
        let mut optional = [
            retries,
            eta,
            rate,
            Some(Span::styled(format!(" {bytes} "), muted)),
            protocol,
        ];

        let fixed: usize =
            head.iter().map(Span::width).sum::<usize>() + percent.width() + CANCEL.chars().count();
        let optional_width = |optional: &[Option<Span>]| -> usize {
            optional.iter().flatten().map(Span::width).sum()
        };
        // the gauge first: the optional parts go until its smallest one fits
        let gauge_min = if size.is_some() { GAUGE_MIN + 2 } else { 0 };
        for i in 0..optional.len() {
            if fixed + optional_width(&optional) + gauge_min <= width {
                break;
            }
            optional[i] = None;
        }
        let room = width.saturating_sub(fixed + optional_width(&optional) + 2);
        let [retries, eta, rate, bytes, protocol] = optional;

        spans = head;
        spans.extend(protocol);
        if size.is_some() && room >= GAUGE_MIN {
            spans.extend(gauge(
                transfer.percent(),
                room.min(GAUGE_MAX),
                accent,
                muted,
            ));
        }
        spans.push(percent);
        spans.extend([bytes, rate, eta, retries].into_iter().flatten());
    }

    let used: usize = spans.iter().map(Span::width).sum();
    let pad = width.saturating_sub(used + CANCEL.chars().count());
    spans.push(Span::raw(" ".repeat(pad)));
    spans.push(Span::styled(CANCEL, muted));
    spans
}

/// `[████░░░░]`, `width` cells between the brackets
fn gauge(percent: u64, width: usize, filled: Style, empty: Style) -> [Span<'static>; 4] {
    let done = (percent.min(100) as usize * width) / 100;
    [
        Span::styled("[", empty),
        Span::styled("█".repeat(done), filled),
        Span::styled("░".repeat(width - done), empty),
        Span::styled("]", empty),
    ]
}

/// A duration as a clock: `m:ss`, `h:mm:ss` from an hour
pub fn clock(duration: Duration) -> String {
    let secs = duration.as_secs();
    let (hours, minutes, secs) = (secs / 3600, secs / 60 % 60, secs % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}
