//! Golden output of the status bar templates, locked before precompiling them (#10).

use super::*;
use crate::app::search::SearchDirection;
use crate::{
    configs::{KeyBindingsConfig, KeyResolver, SettingsConfig},
    inputs::{Action, AppAction, CopyModeAction, SearchModeAction},
    link::{Address, LinkConfig},
};

fn app(mode: Mode) -> App {
    let mut settings = SettingsConfig::default();
    settings.mode = mode;
    settings.ui.status_bar.time_format = "TIME".to_string();
    let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
    App::new(settings, resolver, LinkConfig::default())
}

/// One "text|fg|bg|modifiers" string per span, so expectations stay readable
fn render(template: &str, app: &App) -> Vec<String> {
    let theme = ThemeConfig::default();
    let time_format = &app.settings().ui.status_bar.time_format;
    CompiledTemplate::new(template, &theme)
        .spans(app, *theme.ui.status_bar, time_format)
        .iter()
        .map(|s| {
            format!(
                "{}|{:?}|{:?}|{:?}",
                s.content, s.style.fg, s.style.bg, s.style.add_modifier
            )
        })
        .collect()
}

const BG: &str = "Some(Rgb(30, 35, 45))";
const MUTED: &str = "Some(Rgb(140, 146, 158))";

fn span(text: &str, fg: &str, bg: &str, mods: &str) -> String {
    format!("{text}|{fg}|{bg}|{mods}")
}

fn plain(text: &str) -> String {
    span(text, "Some(White)", BG, "NONE")
}

/// The default bar's `{port}`: in the link's color for its tag
fn port(text: &str, tag: &str) -> String {
    let color = ThemeConfig::default().palette.link_color(tag);
    span(text, &format!("Some({color:?})"), BG, "NONE")
}

/// Default left template in terminal mode: mode pill, port, baud and framing styles.
/// How: golden spans captured from the code before #10.
#[test]
fn default_left_terminal() {
    let a = app(Mode::Terminal);
    let d = StatusBarConfig::default();
    assert_eq!(
        render(&d.left, &a),
        [
            span(" TERM ", "Some(Cyan)", BG, "BOLD"),
            span("|", MUTED, BG, "NONE"),
            port(" /dev/ttyUSB0", "ttyUSB0"),
            span(" |", MUTED, BG, "NONE"),
            plain(" "),
            span("115200", "Some(Cyan)", BG, "NONE"),
            plain(" "),
            plain("8N1"),
            span(" |", MUTED, BG, "NONE"),
            span(" NOR", MUTED, BG, "NONE"),
        ]
    );
}

/// MAX-205: `{cursor_keys}` is the cursor keys mode the remote set (DECCKM), `NOR` or `APP`
/// like minicom's, and `app_cursor` holds in `APP`; both only in terminal mode, where the
/// arrows go to the remote. The default bar shows it after the framing, muted.
/// How: `ESC[?1h` then `ESC[?1l` received in terminal mode, then `ESC[?1h` in line mode.
#[test]
fn cursor_keys_variable_and_condition() {
    let d = StatusBarConfig::default();
    let template = "[{cursor_keys}](app_cursor?red:blue)";
    let mut t = app(Mode::Terminal);
    assert_eq!(
        render(template, &t),
        [span("NOR", "Some(Blue)", BG, "NONE")]
    );

    t.feed_rx(b"\x1b[?1h");
    assert_eq!(render(template, &t), [span("APP", "Some(Red)", BG, "NONE")]);
    assert_eq!(
        render(&d.left, &t).last().unwrap(),
        &span(" APP", MUTED, BG, "NONE")
    );

    t.feed_rx(b"\x1b[?1l");
    assert_eq!(
        render(template, &t),
        [span("NOR", "Some(Blue)", BG, "NONE")]
    );

    let mut l = app(Mode::Line);
    l.feed_rx(b"\x1b[?1h");
    assert_eq!(render(template, &l), Vec::<String>::new());
    assert_eq!(render("[x](app_cursor?red:hide)", &l), Vec::<String>::new());
    assert_eq!(render(&d.left, &l).last().unwrap(), &plain("8N1"));
}

/// MAX-156: a TCP link has no line settings: `{baud}` is `raw` and `{framing}` the link
/// kind, in the same places and styles; `line_settings` can hide them.
/// How: the default left template with a `tcp://` port, a padded baud, the condition.
#[test]
fn tcp_link_has_no_baud_nor_framing() {
    let mut a = app(Mode::Terminal);
    a.link_mut()
        .set_address("tcp://192.168.1.50:4000".parse::<Address>().unwrap());
    let d = StatusBarConfig::default();
    assert_eq!(
        render(&d.left, &a),
        [
            span(" TERM ", "Some(Cyan)", BG, "BOLD"),
            span("|", MUTED, BG, "NONE"),
            port(" tcp://192.168.1.50:4000", "192.168.1.50_4000"),
            span(" |", MUTED, BG, "NONE"),
            plain(" "),
            span("raw", "Some(Cyan)", BG, "NONE"),
            plain(" "),
            plain("TCP"),
            span(" |", MUTED, BG, "NONE"),
            span(" NOR", MUTED, BG, "NONE"),
        ]
    );
    assert_eq!(render("<{baud:5}>", &a), [plain("<raw  >")]);
    assert_eq!(render("[x](serial?bold:hide)", &a), Vec::<String>::new());
}

/// MAX-166: a replayed file has no line settings: `{port}` is `file://` and its name,
/// `{baud}` is `replay` and `{framing}` its state, `RUN` then `END`, in the same places and
/// styles; `line_settings` hides them.
/// How: the default left template on a `file://` port, then after the end.
#[test]
fn replay_shows_its_file_and_state() {
    let mut a = app(Mode::Terminal);
    // as a replay starts: the replay state comes with the address (MAX-175)
    a.start_replay(std::path::PathBuf::from("/tmp/logs/boot.raw"), None);
    let d = StatusBarConfig::default();
    assert_eq!(
        render(&d.left, &a),
        [
            span(" TERM ", "Some(Cyan)", BG, "BOLD"),
            span("|", MUTED, BG, "NONE"),
            port(" file://boot.raw", "boot.raw"),
            span(" |", MUTED, BG, "NONE"),
            plain(" "),
            span("replay", "Some(Cyan)", BG, "NONE"),
            plain(" "),
            plain("RUN"),
            span(" |", MUTED, BG, "NONE"),
            span(" NOR", MUTED, BG, "NONE"),
        ]
    );
    a.end_replay();
    assert_eq!(render("{baud} {framing}", &a), [plain("replay END")]);
    assert_eq!(render("[x](serial?bold:hide)", &a), Vec::<String>::new());
}

/// MAX-166: the replay's variables: its file, state and progress; `{framing}` says
/// `PAUSE` while paused, `replaying` holds; without a replay they are empty or zero.
/// How: a 10-byte file replayed, 4 bytes read, paused; then an app on a port.
#[test]
fn replay_variables() {
    let path = std::env::temp_dir().join(format!(
        "maxicom_test_{}_status_replay.raw",
        std::process::id()
    ));
    std::fs::write(&path, b"0123456789").unwrap();
    let mut a = app(Mode::Terminal);
    let template = "{replay_file}|{replay_state}|{replay_percent}|{replay_read}|{replay_size}";
    assert_eq!(render(template, &a), [plain("||0|0|0")]);
    assert_eq!(render("[x](replaying?bold:hide)", &a), Vec::<String>::new());

    let replay = crate::link::replay::Replay::open(&path, std::time::Duration::ZERO).unwrap();
    let mut link = crate::link::LinkState::new(a.link_config().clone());
    link.set_replay(replay);
    *a.link_mut() = link;
    a.start_replay(path.clone(), None);
    a.link_mut().read(&mut [0; 4]).unwrap().unwrap();
    a.link_mut().replay_mut().unwrap().pause();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        render(template, &a),
        [plain(&format!("{name}|PAUSE|40|4|10"))]
    );
    assert_eq!(render("{baud} {framing}", &a), [plain("replay PAUSE")]);
    assert_eq!(
        render("[x](replaying?bold:hide)", &a),
        [span("x", "Some(White)", BG, "BOLD")]
    );
    let _ = std::fs::remove_file(&path);
}

/// MAX-166: a replayed serial session shows the line settings it was recorded with, in
/// the baud and framing places and styles; `line_settings` shows them.
/// How: the default left template while replaying a session recorded at 9600 7E1.
#[test]
fn session_replay_shows_its_recorded_line() {
    let mut a = app(Mode::Terminal);
    let mut session = crate::capture::tests::device("/dev/ttyACM1");
    session.line = Some(crate::capture::CaptureLine {
        baud: 9600,
        framing: "7E1".parse().unwrap(),
    });
    a.start_replay(
        std::path::PathBuf::from("/tmp/logs/boot.raw"),
        Some(&session),
    );
    let d = StatusBarConfig::default();
    assert_eq!(
        render(&d.left, &a),
        [
            span(" TERM ", "Some(Cyan)", BG, "BOLD"),
            span("|", MUTED, BG, "NONE"),
            port(" file://boot.raw", "boot.raw"),
            span(" |", MUTED, BG, "NONE"),
            plain(" "),
            span("9600", "Some(Cyan)", BG, "NONE"),
            plain(" "),
            plain("7E1"),
            span(" |", MUTED, BG, "NONE"),
            span(" NOR", MUTED, BG, "NONE"),
        ]
    );
    assert_eq!(render("<{baud:5}>", &a), [plain("< 9600>")]);
    assert_eq!(
        render("[x](serial?bold:hide)", &a),
        [span("x", "Some(White)", BG, "BOLD")]
    );
}

/// The mode pill follows the prefix, copy-mode and search conditions (MAX-193). In copy mode
/// it reads COPY (MAX-208); while a search is typed SRCH, black on light blue, then COPY
/// again once it's confirmed (MAX-218).
/// How: same template with the prefix active, then in line copy mode, then searching, then
/// the search confirmed.
#[test]
fn default_left_conditions() {
    let d = StatusBarConfig::default();
    let mut a = app(Mode::Terminal);
    a.update(Action::App(AppAction::SetPrefix(true)));
    assert_eq!(
        render(&d.left, &a)[0],
        span(" TERM ", "Some(Black)", "Some(Yellow)", "BOLD")
    );

    let mut l = app(Mode::Line);
    l.update(Action::CopyMode(CopyModeAction::Enter));
    assert_eq!(
        render(&d.left, &l)[0],
        span(" COPY ", "Some(Black)", "Some(Magenta)", "BOLD")
    );

    let mut s = app(Mode::Line);
    s.update(Action::Search(SearchModeAction::Start(
        SearchDirection::Forward,
    )));
    assert_eq!(
        render(&d.left, &s)[0],
        span(" SRCH ", "Some(Black)", "Some(LightBlue)", "BOLD")
    );
    s.update(Action::Search(SearchModeAction::Confirm));
    assert_eq!(
        render(&d.left, &s)[0],
        span(" COPY ", "Some(Black)", "Some(Magenta)", "BOLD")
    );
}

/// MAX-71: `{copy_side}` and `copy_raw` follow the copy cursor's panel. MAX-193: the default
/// left template doesn't show the side (the badge's color does), so the bar doesn't move in
/// and out of copy mode; the README's block for it takes the selection colors.
/// How: line mode with a Hex raw panel and one line, copy mode, then the toggle.
#[test]
fn copy_side_variable_and_condition() {
    let d = StatusBarConfig::default();
    let template = "[{copy_side}](copy_raw?red:blue)";
    let readme = "[ {copy_side} ](copy_raw?(fg=selection_raw bold):copy?(fg=selection bold):hide)";
    let mut l = app(Mode::Line);
    l.settings_mut().raw_side.view = crate::configs::RawSideView::Hex;
    l.feed_rx(b"abc\r\n");
    let outside = render(&d.left, &l).len();
    assert_eq!(render(readme, &l), Vec::<String>::new());

    l.update(Action::CopyMode(CopyModeAction::Enter));
    assert_eq!(
        render(template, &l),
        [span("TXT", "Some(Blue)", BG, "NONE")]
    );
    assert_eq!(
        render(readme, &l),
        [span(" TXT ", "Some(Magenta)", BG, "BOLD")]
    );
    assert_eq!(render(&d.left, &l).len(), outside);

    l.update(Action::CopyMode(CopyModeAction::ToggleSide));
    assert_eq!(render(template, &l), [span("RAW", "Some(Red)", BG, "NONE")]);
    assert_eq!(render(readme, &l), [span(" RAW ", "Some(Red)", BG, "BOLD")]);
    assert_eq!(
        render(&d.left, &l)[..2],
        [
            span(" COPY ", "Some(Black)", "Some(Red)", "BOLD"),
            span("|", MUTED, BG, "NONE"),
        ]
    );
}

/// MAX-164: while a mark waits for its letter `{mode}` reads MARK, in copy mode too, and the
/// `mark` condition holds: the default badge is black on the theme's good color.
/// How: a mark waiting from terminal mode, then cancelled; in line copy mode, then a jump.
#[test]
fn mark_badge_while_a_letter_is_awaited() {
    use crate::inputs::{MarkAction, PendingMark};
    let d = StatusBarConfig::default();
    let good = format!("Some({:?})", ThemeConfig::default().palette.text_good);
    let mut t = app(Mode::Terminal);
    assert_eq!(render("[x](mark?red:hide)", &t), Vec::<String>::new());
    t.update(Action::App(AppAction::Mark(MarkAction::Await(
        PendingMark::Add,
    ))));
    assert_eq!(render("{mode}", &t), [plain("MARK")]);
    assert_eq!(
        render("[x](mark_pending?red:hide)", &t),
        [span("x", "Some(Red)", BG, "NONE")]
    );
    assert_eq!(
        render(&d.left, &t)[0],
        span(" MARK ", "Some(Black)", &good, "BOLD")
    );
    t.update(Action::App(AppAction::Mark(MarkAction::Cancel)));
    assert_eq!(render("{mode}", &t), [plain("TERM")]);

    let mut l = app(Mode::Line);
    l.feed_rx(b"abc\r\n");
    l.update(Action::CopyMode(CopyModeAction::Enter));
    l.update(Action::App(AppAction::Mark(MarkAction::Await(
        PendingMark::Jump,
    ))));
    assert_eq!(render("{mode}", &l), [plain("MARK")]);
    l.update(Action::App(AppAction::Mark(MarkAction::Jump('z'))));
    assert_eq!(render("{mode}", &l), [plain("COPY")]);
}

/// MAX-193: the default mode badge takes the theme's selection colors in copy mode, and the
/// search bar label's while searching, so a theme changing `[ui.selection]`,
/// `[ui.selection_raw]` or `[search.search_indicator]` changes it too.
/// How: green and blue selections, line copy mode on the text side then on the raw side;
/// a yellow search label, searching.
#[test]
fn copy_badge_follows_the_selection_colors() {
    use ratatui::style::{Color, Style};
    let mut theme = ThemeConfig::default();
    theme.ui.selection = Style::default().bg(Color::Green).into();
    theme.ui.selection_raw = Style::default().bg(Color::Blue).into();
    let d = StatusBarConfig::default();
    let template = CompiledTemplate::new(&d.left, &theme);
    let badge = |app: &App| {
        let time_format = &app.settings().ui.status_bar.time_format;
        template.spans(app, *theme.ui.status_bar, time_format)[0]
            .style
            .bg
    };
    let mut l = app(Mode::Line);
    l.settings_mut().raw_side.view = crate::configs::RawSideView::Hex;
    l.feed_rx(b"abc\r\n");
    l.update(Action::CopyMode(CopyModeAction::Enter));
    assert_eq!(badge(&l), Some(Color::Green));
    l.update(Action::CopyMode(CopyModeAction::ToggleSide));
    assert_eq!(badge(&l), Some(Color::Blue));

    // and the search bar label's while searching
    theme.search.search_indicator = Style::default().bg(Color::Yellow).into();
    let template = CompiledTemplate::new(&d.left, &theme);
    let mut s = app(Mode::Line);
    s.update(Action::Search(SearchModeAction::Start(
        SearchDirection::Forward,
    )));
    let time_format = &s.settings().ui.status_bar.time_format;
    let spans = template.spans(&s, *theme.ui.status_bar, time_format);
    assert_eq!(spans[0].style.bg, Some(Color::Yellow));
}

/// `mouse` is true while the mouse capture setting is on (MAX-113).
#[test]
fn mouse_condition() {
    let mut l = app(Mode::Line);
    assert_eq!(
        render("[m](mouse?red:blue)", &l),
        [span("m", "Some(Red)", BG, "NONE")]
    );
    l.settings_mut().ui.mouse = false;
    assert_eq!(
        render("[m](mouse?red:blue)", &l),
        [span("m", "Some(Blue)", BG, "NONE")]
    );
}

/// MAX-163: `\` at the end of a line joins the next one, its indentation (spaces, tabs)
/// dropped, with LF or CRLF; a space before the `\` stays; another `\` stays as written.
/// The default templates, written one block per line, join into one line.
/// How: the same template on one line and on three, rendered; the joins alone.
#[test]
fn templates_join_lines_ending_in_backslash() {
    let a = app(Mode::Line);
    let one = "[a](red) [b](blue)[c](green)";
    let three = "[a](red) \\\n    [b](blue)\\\r\n\t[c](green)";
    assert_eq!(render(three, &a), render(one, &a));
    assert_eq!(join_lines("a\\b\\"), "a\\b\\");
    assert_eq!(join_lines("a\\\n\n  b"), "a\n  b");

    let defaults = StatusBarConfig::default();
    for template in [&defaults.left, &defaults.center, &defaults.right] {
        assert!(template.contains("\\\n"));
        let joined = join_lines(template);
        assert!(!joined.contains(['\\', '\n']), "{joined}");
    }
}

/// MAX-147: `session` is true while the session backup runs, which never lights `rec`.
/// How: a backup started in a temp folder.
#[test]
fn session_condition() {
    let mut a = app(Mode::Line);
    let template = "[s](session?red:blue)[r](rec?red:blue)";
    assert_eq!(
        render(template, &a),
        [
            span("s", "Some(Blue)", BG, "NONE"),
            span("r", "Some(Blue)", BG, "NONE")
        ]
    );
    let dir =
        std::env::temp_dir().join(format!("maxicom_test_{}_session_cond", std::process::id()));
    a.start_session(Some(dir.clone()));
    assert_eq!(
        render(template, &a),
        [
            span("s", "Some(Red)", BG, "NONE"),
            span("r", "Some(Blue)", BG, "NONE")
        ]
    );
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Default center and right templates: hidden blocks are omitted, counters and time filled.
/// How: golden spans in terminal mode, time format set to a literal.
#[test]
fn default_center_and_right() {
    let a = app(Mode::Terminal);
    let d = StatusBarConfig::default();
    assert!(render(&d.center, &a).is_empty());
    assert_eq!(
        render(&d.right, &a),
        [
            span("RX:    0", MUTED, BG, "NONE"),
            span(" | ", MUTED, BG, "NONE"),
            span("TX:    0", MUTED, BG, "NONE"),
            span(" | ", MUTED, BG, "NONE"),
            span("TIME", "Some(Yellow)", BG, "NONE"),
            plain(" "),
            span("?", MUTED, BG, "NONE"),
            plain(" "),
            span("≡", MUTED, BG, "NONE"),
            plain(" "),
        ]
    );
}

/// MAX-6: the default center shows how many triggers are armed, hidden while none is;
/// `{trigger_names}` names them, the first and how many more when squeezed; a click on
/// either opens the trigger browser (MAX-167).
/// How: the default triggers, disarmed then all armed, then one.
#[test]
fn armed_triggers() {
    let mut a = app(Mode::Terminal);
    let triggers = crate::configs::triggers::tests::two_examples()
        .compile()
        .unwrap();
    a.set_triggers(triggers);
    let d = StatusBarConfig::default();
    let texts = |a: &App| -> Vec<String> {
        let spans = render(&d.center, a);
        spans
            .iter()
            .map(|s| s.split('|').next().unwrap().to_string())
            .collect()
    };
    let compact = Fit {
        compact: true,
        drop_from: None,
    };
    assert!(texts(&a).is_empty());
    assert_eq!(text_with("{trigger_names}", &a, compact), "");

    a.update(Action::App(AppAction::ToggleTriggers));
    assert_eq!(texts(&a), ["◎ 2 "]);
    assert_eq!(render("{trigger_names}", &a), [plain("login, u-boot")]);
    assert_eq!(text_with("{trigger_names}", &a, compact), "login +1");
    assert_eq!(render("{trigger_count:02}", &a), [plain("02")]);

    a.update(Action::App(AppAction::ArmTriggers {
        on: false,
        name: Some("login".to_string()),
    }));
    assert_eq!(text_with("{trigger_names}", &a, compact), "u-boot");
    for var in [Var::TriggerCount, Var::TriggerNames] {
        assert_eq!(
            var.click(),
            Some(ClickAction::Run(KeyBindingAction::Triggers))
        );
    }
}

/// MAX-222: the default center shows the plot shown, `{plot}`, in the secondary color, in
/// the warning one while frozen (`frozen`), hidden without a plot (`plot`); a click on the
/// name freezes it.
/// How: no plot, `serial-plotter` shown, frozen, then off.
#[test]
fn plot_shown_and_frozen() {
    let mut a = app(Mode::Terminal);
    a.set_plots(crate::configs::PlotsConfig::default());
    let center = StatusBarConfig::default().center;
    let theme = ThemeConfig::default();
    assert!(render(&center, &a).is_empty());
    assert_eq!(render("{plot}", &a), [plain("")]);

    a.show_plot(Some("serial-plotter"));
    let secondary = format!("Some({:?})", theme.palette.secondary);
    assert_eq!(
        render(&center, &a),
        [span("∿ serial-plotter ", &secondary, BG, "NONE")]
    );
    assert_eq!(render("{plot}", &a), [plain("serial-plotter")]);
    assert_eq!(render("[x](frozen?bold:hide)", &a), Vec::<String>::new());

    a.update(Action::App(AppAction::TogglePlotFreeze));
    let warn = format!("Some({:?})", theme.palette.text_warn);
    assert_eq!(
        render(&center, &a),
        [span("∿ serial-plotter ", &warn, BG, "BOLD")]
    );

    a.show_plot(None);
    assert!(render(&center, &a).is_empty());
    assert_eq!(
        Var::Plot.click(),
        Some(ClickAction::Run(KeyBindingAction::PlotFreeze))
    );
}

/// Every variable is filled; an unknown variable stays as written.
/// How: one template with all variables, one with `{unknown}`.
#[test]
fn variables() {
    let a = app(Mode::Terminal);
    assert_eq!(
        render(
            "[ {mode} {port} {baud} {framing} {rx_bytes} {tx_bytes} {time}](reset)",
            &a
        ),
        [plain(" TERM /dev/ttyUSB0 115200 8N1 0 0 TIME")]
    );
    assert_eq!(
        render(
            "{scroll_pos}/{scroll_pos_up}/{scroll_percent}/{scroll_offset}/{scroll_max}",
            &a
        ),
        [plain("All/ALL/100/0/0")]
    );
    assert_eq!(
        render("plain {unknown} text", &a),
        [plain("plain {unknown} text")]
    );
}

/// Style words: modifiers, explicit colors, unknown words ignored, `hide` drops the span.
/// How: four styled blocks in one template.
#[test]
fn style_words() {
    let a = app(Mode::Terminal);
    assert_eq!(
        render("[a](bold) [b](nope) [c](fg=#112233 bg=red) [d](hide)", &a),
        [
            span("a", "Some(White)", BG, "BOLD"),
            plain(" "),
            plain("b"),
            plain(" "),
            span("c", "Some(Rgb(17, 34, 51))", "Some(Red)", "NONE"),
            plain(" "),
        ]
    );
}

/// Nested ternaries resolve, a ternary without `:` is ignored (base style).
/// How: `term` is false while the prefix is active, so the outer false branch wins.
#[test]
fn ternaries() {
    let mut a = app(Mode::Terminal);
    a.update(Action::App(AppAction::SetPrefix(true)));
    assert_eq!(
        render("[x](term?(copy?red:green):blue) [y](line?red)", &a),
        [span("x", "Some(Blue)", BG, "NONE"), plain(" "), plain("y")]
    );
    let a = app(Mode::Terminal);
    assert_eq!(
        render("[x](term?(copy?red:green):blue)", &a),
        [span("x", "Some(Green)", BG, "NONE")]
    );
}

/// A variable's value is never read as markup, since templates are parsed before filling.
/// How: a port name that looks like a styled block is shown as written.
#[test]
fn variable_value_is_not_markup() {
    let mut a = app(Mode::Terminal);
    a.link_mut()
        .set_address("[x](bold)".parse::<Address>().unwrap());
    assert_eq!(render("port {port}", &a), [plain("port [x](bold)")]);
}

/// Braces around a known variable: only the inner `{mode}` is filled.
/// How: `{{mode}}` and an unclosed `{mode`.
#[test]
fn braces_edge_cases() {
    let a = app(Mode::Terminal);
    assert_eq!(render("{{mode}} {mode", &a), [plain("{TERM} {mode")]);
}

/// Unfinished markup is kept as text, and an empty block renders nothing.
/// How: an unclosed `[`, a `](` without closing paren, and `[](bold)`.
#[test]
fn malformed_markup() {
    let a = app(Mode::Terminal);
    assert_eq!(
        render("open [ bracket and ](x", &a),
        [plain("open "), plain("["), plain(" bracket and ](x")]
    );
    assert_eq!(
        render("[](bold)[empty text above]", &a),
        [plain("["), plain("empty text above]")]
    );
}

/// Where a template's clickable blocks are (start column, width) and what they run
/// A replay over a live link has a `« live` button in the default center part, whose click
/// goes back to that link (`toggle-attach`); none without a replay.
/// How: the default center on the live link, then replaying a file over it.
#[test]
fn replay_has_a_back_to_live_button() {
    let d = StatusBarConfig::default();
    let mut a = app(Mode::Terminal);
    let live = |a: &App| render(&d.center, a).iter().any(|s| s.starts_with("« live"));
    let back = |a: &App| {
        clicks(&d.center, a)
            .iter()
            .any(|(_, _, click)| *click == ClickAction::Run(KeyBindingAction::ToggleAttach))
    };
    assert!(!live(&a) && !back(&a));
    a.start_replay(std::path::PathBuf::from("/tmp/logs/boot.raw"), None);
    assert!(live(&a), "{:?}", render(&d.center, &a));
    // a column before what comes next, the pan arrows
    let center = render(&d.center, &a);
    assert!(
        center.iter().any(|s| s.starts_with("« live ")),
        "{center:?}"
    );
    assert!(back(&a));
}

/// A flag of the default bar's center keeps a column before what comes next: the other flags or
/// the pan arrows.
/// How: +CR and +LF on, in the bar's center.
#[test]
fn center_flags_keep_a_space() {
    let d = StatusBarConfig::default();
    let mut a = app(Mode::Terminal);
    a.settings_mut().add_cr = true;
    a.settings_mut().add_lf = true;
    let center = render(&d.center, &a);
    for flag in ["+CR ", "+LF "] {
        assert!(center.iter().any(|s| s.starts_with(flag)), "{center:?}");
    }
}

/// The theme's colors a template can name: the frame's, the bar's background, the search
/// highlights', and the current link's port color in the session browser, as fg or bg.
/// How: one block per name on the default theme and link (`/dev/ttyUSB0`).
#[test]
fn theme_color_names() {
    use ratatui::style::Color;
    let theme = ThemeConfig::default();
    let a = app(Mode::Line);
    let colors = |template: &str| -> (Option<Color>, Option<Color>) {
        let compiled = CompiledTemplate::new(template, &theme);
        let spans = compiled.spans(&a, *theme.ui.status_bar, "TIME");
        (spans[0].style.fg, spans[0].style.bg)
    };
    assert_eq!(colors("[x](border)").0, theme.ui.border_panel.fg);
    assert_eq!(colors("[x](fg=bar)").0, theme.ui.status_bar.bg);
    assert_eq!(colors("[x](bg=match)").1, theme.search.search_match.bg);
    assert_eq!(
        colors("[x](bg=match_active)").1,
        theme.search.search_match_active.bg
    );
    let port = theme.palette.link_color("ttyUSB0");
    assert_eq!(colors("[{port}](link)").0, Some(port));
    assert_eq!(
        colors("[{port}](bg=link fg=black)"),
        (Some(Color::Black), Some(port))
    );
}

/// MAX-201: the status bar's `link` is the link's saved color, for both fg and bg; the palette's
/// without one, or with a saved link of another address.
/// How: the app's port saved with a color, then another port saved with one.
#[test]
fn link_style_takes_the_saved_color() {
    let saved = |address: &str| {
        let text = format!("[[link]]\nname = 'x'\naddress = '{address}'\ncolor = '#102030'\n");
        Ok(toml::from_str(&text).unwrap())
    };
    let theme = ThemeConfig::default();
    let colors = |app: &App, template: &str| {
        let compiled = CompiledTemplate::new(template, &theme);
        let (spans, _) =
            compiled.spans_and_clicks(app, *theme.ui.status_bar, "TIME", Fit::default());
        (spans[0].style.fg, spans[0].style.bg)
    };
    let color = Color::Rgb(0x10, 0x20, 0x30);
    let mut a = app(Mode::Terminal);
    let address = LinkConfig::default().address.to_string();
    a.set_links(saved(&address));
    assert_eq!(colors(&a, "[x](link)").0, Some(color));
    assert_eq!(colors(&a, "[x](bg=link)").1, Some(color));
    a.set_links(saved("/dev/other"));
    let tag = LinkConfig::default().address.file_tag();
    assert_eq!(
        colors(&a, "[x](link)").0,
        Some(theme.palette.link_color(&tag))
    );
}

fn clicks(template: &str, app: &App) -> Vec<(u16, u16, ClickAction)> {
    spots(template, app)
        .into_iter()
        .map(|(col, width, click, _)| (col, width, click))
        .collect()
}

fn spots(template: &str, app: &App) -> Vec<ClickSpot> {
    let theme = ThemeConfig::default();
    CompiledTemplate::new(template, &theme)
        .spans_and_clicks(app, *theme.ui.status_bar, "TIME", Fit::default())
        .1
}

/// MAX-125: `click=<action>` makes a block clickable and is no style word; an unknown
/// action is dropped and a hidden block isn't clickable.
/// How: the same block with and without `click=help`, `click=nope`, a `hide` one.
#[test]
fn click_keyword() {
    use KeyBindingAction::{Help, Settings};
    let a = app(Mode::Terminal);
    assert_eq!(
        render("[ ? ](muted bold click=help)", &a),
        render("[ ? ](muted bold)", &a)
    );
    assert_eq!(
        clicks("ab [ ? ](muted click=help)[x](click=settings)", &a),
        [
            (3, 3, ClickAction::Run(Help)),
            (6, 1, ClickAction::Run(Settings))
        ]
    );
    assert_eq!(clicks("[ ? ](click=nope)", &a), []);
    assert_eq!(clicks("[ ? ](hide click=help)", &a), []);
}

/// MAX-125: a block takes the click its first variable implies, unless it has its own
/// `click=`; in plain text only the variable is clickable, not the text around it.
/// How: `{mode}`, `{port}` (no click) and `{baud}` in plain text, `{rx_bytes}` (no click),
/// `{copy_side}`.
#[test]
fn variables_imply_clicks() {
    use KeyBindingAction::{Help, LinkSettings, SKInputMode};
    let mut a = app(Mode::Terminal);
    a.link_mut()
        .set_address("/dev/x".parse::<Address>().unwrap());
    let run = ClickAction::Run;
    assert_eq!(clicks("[ {mode} ](bold)", &a), [(0, 6, run(SKInputMode))]);
    assert_eq!(clicks("[{mode}](click=help)", &a), [(0, 4, run(Help))]);
    assert_eq!(
        clicks("[{mode}](click=nope)", &a),
        [(0, 4, run(SKInputMode))]
    );
    let baud = a.link_config().line.baud.to_string().len() as u16;
    assert_eq!(
        clicks("port {port} | {baud} {rx_bytes}", &a),
        [(14, baud, run(LinkSettings))]
    );
    assert_eq!(clicks("[RX {rx_bytes}](muted)", &a), []);

    a.update(Action::CopyMode(CopyModeAction::Enter));
    assert_eq!(
        clicks("[{copy_side}](bold)", &a),
        [(0, 3, ClickAction::ToggleSide)]
    );
}

/// MAX-131: a block's `hover=` is no style word: its own style, `none` to have none, or
/// nothing to take the bar's `hover` option; the option is read the same way.
/// How: `hover=(bg=red bold)`, `hover=none`, no `hover=`, and `compile_hover` alone.
#[test]
fn hover_keyword_and_option() {
    let a = app(Mode::Terminal);
    let theme = ThemeConfig::default();
    assert_eq!(
        render("[ ? ](muted hover=(bg=red bold) click=help)", &a),
        render("[ ? ](muted)", &a)
    );
    let red_bold = Style::default().bg(Color::Red).add_modifier(Modifier::BOLD);
    let hover = |template| spots(template, &a)[0].3;
    assert_eq!(
        hover("[?](click=help hover=(bg=red bold))"),
        Some(Some(red_bold))
    );
    assert_eq!(hover("[?](click=help hover=none)"), Some(None));
    assert_eq!(hover("[?](click=help)"), None);

    let reversed = Style::default().add_modifier(Modifier::REVERSED);
    assert_eq!(compile_hover("reversed", &theme), Some(reversed));
    assert_eq!(compile_hover(" (bg=red bold) ", &theme), Some(red_bold));
    assert_eq!(compile_hover("none", &theme), None);
}

/// MAX-139: `{name:spec}` pads a variable: numbers right and text left by default, `<` `>`
/// `^` to align, `0` for zeros (numbers only), `h` for a human size; a longer value isn't
/// cut, and a malformed spec stays as written.
/// How: the baud rate (115200) and the port with each spec, RX at 1536 bytes.
#[test]
fn variable_formats() {
    let mut a = app(Mode::Terminal);
    for (template, out) in [
        ("{baud:8}", "  115200"),
        ("{baud:08}", "00115200"),
        ("{baud:<8}", "115200  "),
        ("{baud:^9}", " 115200  "),
        ("{baud:3}", "115200"),
        ("{port:14}", "/dev/ttyUSB0  "),
        ("{port:>14}", "  /dev/ttyUSB0"),
        ("{port:014}", "/dev/ttyUSB0  "),
        ("{baud:x}", "{baud:x}"),
        ("{baud:}", "{baud:}"),
        ("{baud:8x}", "{baud:8x}"),
        ("{baud:+8}", "{baud:+8}"),
    ] {
        assert_eq!(render(template, &a), [plain(out)], "{template}");
    }
    a.stats.update_rx(1536);
    assert_eq!(render("{rx_bytes:h}", &a), [plain("1.5K")]);
    assert_eq!(render("{rx_bytes:>6h}", &a), [plain("  1.5K")]);
    assert_eq!(render("{rx_bytes:06}", &a), [plain("001536")]);
}

/// MAX-139: a human size is the number up to 999, then one decimal below 10 and none above,
/// in units of 1024, never more than 4 columns.
#[test]
fn human_sizes() {
    let human = |bytes| {
        let mut out = String::new();
        write_human(&mut out, bytes);
        out
    };
    for (bytes, out) in [
        (0, "0"),
        (999, "999"),
        (1000, "1.0K"),
        (1023, "1.0K"),
        (1024, "1.0K"),
        (1536, "1.5K"),
        (10 * 1024, "10K"),
        (999 * 1024, "999K"),
        (1024 * 1024 - 1, "1.0M"),
        (5_000_000, "4.8M"),
        (u64::MAX, "16E"),
    ] {
        assert_eq!(human(bytes), out, "{bytes}");
    }
}

/// MAX-139: a padded variable is clickable over its whole width.
#[test]
fn padded_variable_click() {
    use KeyBindingAction::LinkSettings;
    let a = app(Mode::Terminal);
    assert_eq!(
        clicks("{baud:10}", &a),
        [(0, 10, ClickAction::Run(LinkSettings))]
    );
}

/// Text of a template's spans in one string, squeezed by `fit`
fn text_with(template: &str, app: &App, fit: Fit) -> String {
    let theme = ThemeConfig::default();
    CompiledTemplate::new(template, &theme)
        .spans_and_clicks(app, *theme.ui.status_bar, "TIME", fit)
        .0
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// MAX-139: `prio=N` is no style word; squeezed, the blocks of a level and above are left
/// out, the ones without `prio=` never; the bar's levels are listed from the highest.
/// How: a template with prio 1, 2, 3 and none, then `drop_from` 3, 2 and 1.
#[test]
fn prio_keyword() {
    let a = app(Mode::Terminal);
    assert_eq!(render("[a](bold prio=2)", &a), render("[a](bold)", &a));
    let template = "[a](prio=1)[b](prio=2)[c]()[d](prio=3)[e](prio=x)";
    let drop = |from| Fit {
        compact: false,
        drop_from: from,
    };
    assert_eq!(text_with(template, &a, drop(None)), "abcde");
    assert_eq!(text_with(template, &a, drop(Some(3))), "abce");
    assert_eq!(text_with(template, &a, drop(Some(2))), "ace");
    assert_eq!(text_with(template, &a, drop(Some(1))), "ce");

    let config = StatusBarConfig {
        left: "[a](prio=2)[b](prio=4)".into(),
        center: String::new(),
        right: "[c](prio=2)[d](prio=1)".into(),
        ..StatusBarConfig::default()
    };
    let bar = CompiledStatusBar::new(&config, &ThemeConfig::default());
    assert_eq!(bar.prios, [4, 2, 1]);
}

/// MAX-139: squeezed, a variable without a format of its own takes its compact form: the
/// port's file name, a byte count as a size, the pan without spaces and parentheses; one
/// with a format keeps it.
/// How: the port, RX at 1536 with and without `:6`, a 200-column terminal panned by 3.
#[test]
fn compact_forms() {
    use crate::{configs::TerminalDimensionMode, inputs::HistoryAction};
    let compact = Fit {
        compact: true,
        drop_from: None,
    };
    let mut a = app(Mode::Terminal);
    a.stats.update_rx(1536);
    assert_eq!(text_with("{port}", &a, Fit::default()), "/dev/ttyUSB0");
    assert_eq!(text_with("{port}", &a, compact), "ttyUSB0");
    assert_eq!(text_with("{rx_bytes}", &a, compact), "1.5K");
    assert_eq!(text_with("{rx_bytes:6}", &a, compact), "  1536");

    a.settings_mut().terminal_mode.dimension_mode = TerminalDimensionMode::Fixed;
    a.settings_mut().terminal_mode.cols = 200;
    a.update_viewport(crate::app::types::ViewportSize::new(80, 20));
    a.update(Action::History(HistoryAction::PanRight(3)));
    assert_eq!(text_with("{pan_left}", &a, Fit::default()), "◀ (3)");
    assert_eq!(text_with("{pan_left}", &a, compact), "◀3");
    assert_eq!(text_with("{pan_right}", &a, compact), "117▶");
}

/// MAX-185: a link saved for this machine shows by its name, full or squeezed; one saved
/// for another machine by its address.
/// How: the app's `/dev/ttyUSB0` saved as `esp32`, then saved for another machine only.
#[test]
fn saved_link_by_name() {
    let compact = Fit {
        compact: true,
        drop_from: None,
    };
    let mut a = app(Mode::Terminal);
    let saved = |hosts: &str| {
        let text = format!("[[link]]\nname = 'esp32'\naddress = '/dev/ttyUSB0'\n{hosts}");
        Ok(toml::from_str(&text).unwrap())
    };
    a.set_links(saved(""));
    assert_eq!(text_with("{port}", &a, Fit::default()), "esp32");
    assert_eq!(text_with("{port}", &a, compact), "esp32");
    a.set_links(saved("hosts = ['not-this-machine-0']"));
    assert_eq!(text_with("{port}", &a, Fit::default()), "/dev/ttyUSB0");
}

/// MAX-181: `add_cr` / `add_lf` hold while their setting is on, in both modes.
#[test]
fn add_cr_add_lf_conditions() {
    for mode in [Mode::Terminal, Mode::Line] {
        let mut app = app(mode);
        assert!(!Cond::AddCr.eval(&app) && !Cond::AddLf.eval(&app));
        app.settings_mut().add_cr = true;
        assert!(
            Cond::AddCr.eval(&app) && !Cond::AddLf.eval(&app),
            "{mode:?}"
        );
        app.settings_mut().add_lf = true;
        assert!(Cond::AddLf.eval(&app), "{mode:?}");
    }
    assert_eq!(Cond::from_name("add_cr"), Cond::AddCr);
    assert_eq!(Cond::from_name("ADD_LF"), Cond::AddLf);
}
