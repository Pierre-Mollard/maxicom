use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::App,
    configs::{
        AppConfig, PrefixWrapBehavior, ThemeConfig, keybindings::KeyBindingDialogAction,
        settings::PrefixPlace,
    },
    overlays::{PopupTarget, SettingKey, SettingsDialog},
    paths::shorten_home,
    ui::popups::{
        FORM_WIDTH, form_area, key_hints, popup_block, render_scrollbar, row_area, section,
    },
};

/// The settings in their groups, each row a click target, the config files under them; in
/// two columns when the screen has room (MAX-184), scrolled when it's too short
pub fn render_settings_popup(
    app: &App,
    dialog: &SettingsDialog,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let mode_str = match app.settings().mode {
        crate::configs::Mode::Line => "Line",
        crate::configs::Mode::Terminal => "Terminal",
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
    let prefix_place_str = match app.settings().line_mode.prefix_place {
        PrefixPlace::Inset => "Inset",
        PrefixPlace::Border => "Border",
        PrefixPlace::None => "None",
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
            Span::raw(format!("{label:<14}: ")),
            // padded to the longest value, so a column keeps its width as values change
            Span::styled(
                format!("{value:<VALUE_WIDTH$}"),
                Style::default().fg(theme.palette.main),
            ),
        ])
    };

    let mouse_str = if app.settings().ui.mouse { "ON" } else { "OFF" };
    let answers_str = if app.settings().terminal_mode.answer_queries {
        "ON"
    } else {
        "OFF"
    };
    let on_off = |on: bool| if on { "ON" } else { "OFF" };
    let add_cr_str = on_off(app.settings().add_cr);
    let add_lf_str = on_off(app.settings().add_lf);
    let tx_eol_str = app.settings().line_mode.tx_eol.as_str();
    // each row with the setting a click on it toggles (MAX-113), in groups
    let group = |title, rows: Vec<(SettingKey, Line<'static>)>| {
        let mut group = vec![(None, section(title, theme))];
        group.extend((rows.into_iter()).map(|(key, line)| (Some(PopupTarget::Setting(key)), line)));
        group
    };
    let general = group(
        "General",
        vec![
            (SettingKey::Mode, row("M", "Input Mode", mode_str)),
            (SettingKey::LocalEcho, row("E", "Local Echo", echo_str)),
            (
                SettingKey::DisplayTimestamp,
                row("T", "Timestamps", timestamp_str),
            ),
            (
                SettingKey::TextEncoding,
                row("X", "Rx Encoding", encoding_str),
            ),
            (
                SettingKey::DisplayRaw,
                row("B", "Byte Inspect", raw_display_str),
            ),
            (SettingKey::Mouse, row("U", "Mouse", mouse_str)),
        ],
    );
    // what's received (both modes) and what Enter sends (line mode, MAX-184)
    let newlines = group(
        "Newlines",
        vec![
            (SettingKey::AddCr, row("R", "Add CR", add_cr_str)),
            (SettingKey::AddLf, row("F", "Add LF", add_lf_str)),
            (SettingKey::TxEol, row("N", "Line Enter", tx_eol_str)),
        ],
    );
    let line_mode = group(
        "Line Mode",
        vec![
            (SettingKey::LineWrap, row("W", "Line Wrap", wrap_str)),
            (SettingKey::IdentWrap, row("I", "Indent Wrap", indent_str)),
            (
                SettingKey::PrefixPlace,
                row("P", "Prefix Place", prefix_place_str),
            ),
            (
                SettingKey::DisplayMode,
                row("D", "Display Mode", display_mode_str),
            ),
        ],
    );
    let terminal_mode = group(
        "Terminal Mode",
        vec![
            (
                SettingKey::DimensionMode,
                row("S", "Size Mode", dimension_str),
            ),
            (
                SettingKey::AnswerQueries,
                row("A", "Query Answers", answers_str),
            ),
        ],
    );
    // the config folder, then the files out of it; a click copies one (MAX-163)
    let mut files = vec![(None, section("Files", theme))];
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
            Span::raw(format!("{label:<14}: ")),
            Span::styled(shorten_home(&path), Style::default().fg(theme.palette.main)),
        ]);
        files.push((Some(PopupTarget::ConfigPath(index)), line));
    }

    // General and Newlines on the left, Line Mode and Terminal Mode on the right, when the
    // screen has room (MAX-184); else one column, as before
    let blank = || (None, Line::from(""));
    let column = |top: Vec<Row>, bottom: Vec<Row>| {
        let mut column = top;
        column.push(blank());
        column.extend(bottom);
        column
    };
    let (left, right) = (column(general, newlines), column(line_mode, terminal_mode));
    let widest = |rows: &[Row]| rows.iter().map(|(_, line)| line.width()).max().unwrap_or(0);
    let left_width = widest(&left);
    let two_width = (left_width + GAP + widest(&right)) as u16 + 4;
    let mut rows: Vec<(Vec<Spot>, Line)> = Vec::new();
    let whole =
        |target: Option<PopupTarget>| target.map(|t| (0, u16::MAX, t)).into_iter().collect();
    let width = if frame.area().width >= two_width {
        let start = (left_width + GAP) as u16;
        let (mut left, mut right) = (left.into_iter(), right.into_iter());
        loop {
            let (left, right) = match (left.next(), right.next()) {
                (None, None) => break,
                (left, right) => (left.unwrap_or_else(blank), right.unwrap_or_else(blank)),
            };
            let ((left_target, mut line), (right_target, right_line)) = (left, right);
            let pad = (left_width + GAP).saturating_sub(line.width());
            line.spans.push(Span::raw(" ".repeat(pad)));
            line.spans.extend(right_line.spans);
            // a left row's target stops a column before the right one
            let left_spot = left_target.map(|t| (0, start - 1, t));
            let mut spots: Vec<Spot> = left_spot.into_iter().collect();
            spots.extend(right_target.map(|t| (start, u16::MAX, t)));
            rows.push((spots, line));
        }
        two_width
    } else {
        for (target, line) in column(left, right) {
            rows.push((whole(target), line));
        }
        FORM_WIDTH
    };
    rows.push((Vec::new(), Line::from("")));
    for (target, line) in files {
        rows.push((whole(target), line));
    }

    let hints = key_hints(
        &app.key_resolver,
        false,
        &[(&[KeyBindingDialogAction::Close], "Return")],
        theme,
    );
    let block = popup_block("Settings", &hints, theme);

    let area = form_area(frame.area(), width, rows.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // too short for every row: scrolled, a scrollbar in the right padding (MAX-163)
    let visible = inner.height as usize;
    let max_scroll = rows.len().saturating_sub(visible);
    dialog.max_scroll.set(max_scroll);
    let scroll = dialog.scroll.min(max_scroll);
    for (y, (spots, _)) in rows.iter().enumerate().skip(scroll).take(visible) {
        let row = row_area(inner, y - scroll);
        for (x, width, target) in spots {
            let spot = Rect {
                x: row.x.saturating_add(*x),
                width: *width,
                ..row
            };
            targets.push((spot.intersection(row), *target));
        }
    }
    hints.push_block_targets(area, targets);
    let text: Vec<Line> = rows.into_iter().map(|(_, line)| line).collect();
    frame.render_widget(Paragraph::new(text).scroll((scroll as u16, 0)), inner);
    if max_scroll > 0 {
        let bar = Rect {
            x: inner.right(),
            width: 1,
            ..inner
        }
        .intersection(area);
        render_scrollbar(frame, bar, (scroll, max_scroll, visible), theme);
    }
    area
}

/// A row of the popup and the setting or file a click on it picks
type Row = (Option<PopupTarget>, Line<'static>);

/// Where a click picks a target on a row: its first column and its width
type Spot = (u16, u16, PopupTarget);

/// Between the two columns
const GAP: usize = 4;

/// The longest setting value: `Match View`, `Fixed Cols`
const VALUE_WIDTH: usize = 10;
