use super::*;

fn parser(def: PlotDef) -> PlotParser {
    PlotParser::new(&def)
}

fn separator(separator: &str) -> PlotDef {
    PlotDef {
        separator: separator.to_string(),
        ..PlotDef::default()
    }
}

fn arduino() -> PlotDef {
    PlotDef {
        separator: ":".to_string(),
        columns: true,
        window: PlotWindow::Samples(500),
        ..PlotDef::default()
    }
}

fn secs(secs: f64) -> Duration {
    Duration::from_secs_f64(secs)
}

fn points(data: &PlotData, name: &str) -> Vec<(f64, f64)> {
    let series = data.series().iter().find(|series| series.name == name);
    series.unwrap().points().iter().copied().collect()
}

fn names(data: &PlotData) -> Vec<&str> {
    data.series()
        .iter()
        .map(|series| series.name.as_str())
        .collect()
}

#[test]
fn every_separator() {
    let cases = [
        ("=", "temp=23.4 hum=51"),
        ("=", "temp = 23.4, hum = 51"),
        (":", "temp:23.4,hum:51"),
        (":", "temp: 23.4\thum:\t51"),
        ("->", "temp -> 23.4 | hum->51"),
        ("=>", "[sensor] temp => 23.4; hum=>51"),
    ];
    for (sep, line) in cases {
        assert_eq!(
            parser(separator(sep)).parse(line),
            [("temp", 23.4), ("hum", 51.0)],
            "{sep:?} in {line:?}"
        );
    }
}

/// The word right before the separator, nothing before it: a key made of letters, digits,
/// `_` and `.`, any text in front.
#[test]
fn key_is_the_word_before() {
    let parser = parser(separator("="));
    assert_eq!(
        parser.parse("[12:00] imu.ax=0.5 adc_3=1023 (pid)err=-2"),
        [("imu.ax", 0.5), ("adc_3", 1023.0), ("err", -2.0)]
    );
    assert_eq!(parser.parse("=5 ( =6"), []);
    assert_eq!(parser.parse("température=21"), [("température", 21.0)]);
}

#[test]
fn numbers() {
    let parser = parser(separator("="));
    let cases = [
        ("v=-1.5e3", -1500.0),
        ("v=+2", 2.0),
        ("v=.5", 0.5),
        ("v=5.", 5.0),
        ("v=1E-2", 0.01),
        ("v=-0", 0.0),
        ("v=007", 7.0),
    ];
    for (line, value) in cases {
        assert_eq!(parser.parse(line), [("v", value)], "{line:?}");
    }
}

/// A unit stuck after the number is left; an `e` without digits is a unit too.
#[test]
fn units_ignored() {
    let parser = parser(separator("="));
    assert_eq!(
        parser.parse("temp=23.4C hum=51% v=3.3V d=12em t=5ms"),
        [
            ("temp", 23.4),
            ("hum", 51.0),
            ("v", 3.3),
            ("d", 12.0),
            ("t", 5.0)
        ]
    );
}

/// A pair whose value isn't a number is skipped, the rest of the line is read. Hex isn't a
/// number.
#[test]
fn junk_values_skipped() {
    let parser = parser(separator("="));
    assert_eq!(
        parser.parse("mode=auto a=1 b= c=- reg=0x1F d=nan e=inf f=2"),
        [("a", 1.0), ("f", 2.0)]
    );
    assert_eq!(parser.parse("boot ok, no numbers here"), []);
    assert_eq!(parser.parse(""), []);
    assert_eq!(parser.parse("a==1"), []);
}

/// A value isn't read again as the next key: `a=1=2` gives `a` only.
#[test]
fn value_not_a_key() {
    let parser = parser(separator("="));
    assert_eq!(parser.parse("a=1=2 b=3"), [("a", 1.0), ("b", 3.0)]);
}

#[test]
fn first_of_a_key_counts() {
    let parser = parser(separator("="));
    assert_eq!(parser.parse("t=1 h=2 t=3"), [("t", 1.0), ("h", 2.0)]);
}

#[test]
fn keys_filter() {
    let parser = parser(PlotDef {
        keys: vec!["hum".to_string(), "temp".to_string()],
        ..PlotDef::default()
    });
    assert_eq!(
        parser.parse("temp=23.4 pres=1013 hum=51"),
        [("temp", 23.4), ("hum", 51.0)]
    );
}

#[test]
fn line_contains() {
    let parser = parser(PlotDef {
        line_contains: Some("PID".to_string()),
        ..PlotDef::default()
    });
    assert_eq!(
        parser.parse("[PID] err=-2 out=40"),
        [("err", -2.0), ("out", 40.0)]
    );
    assert_eq!(parser.parse("[ADC] err=5"), []);
    // case matters
    assert_eq!(parser.parse("[pid] err=5"), []);
}

#[test]
fn arduino_lines() {
    let parser = parser(arduino());
    assert_eq!(
        parser.parse("temp:23.4,hum:51"),
        [("temp", 23.4), ("hum", 51.0)]
    );
    assert_eq!(parser.parse("23.4 51"), [("1", 23.4), ("2", 51.0)]);
    assert_eq!(
        parser.parse("23.4,51,-3e2"),
        [("1", 23.4), ("2", 51.0), ("3", -300.0)]
    );
    assert_eq!(
        parser.parse("  1, 2\t3  "),
        [("1", 1.0), ("2", 2.0), ("3", 3.0)]
    );
    assert_eq!(parser.parse("42"), [("1", 42.0)]);
}

/// Bare columns need numbers only: a unit or a word and the line is read as pairs. Off, a
/// line of numbers gives nothing.
#[test]
fn bare_columns_need_numbers_only() {
    let parser_on = parser(arduino());
    assert_eq!(parser_on.parse("23.4C 51"), []);
    assert_eq!(parser_on.parse("t 1 2"), []);
    assert_eq!(parser_on.parse("1 0x1F"), []);
    assert_eq!(parser(separator(":")).parse("23.4 51"), []);
}

#[test]
fn bare_columns_capped_and_picked() {
    let line = "1 2 3 4 5 6 7 8 9 10";
    let parser_all = parser(arduino());
    let all = parser_all.parse(line);
    assert_eq!(all.len(), MAX_SERIES);
    assert_eq!(all.last(), Some(&("8", 8.0)));

    let picked = parser(PlotDef {
        keys: vec!["2".to_string()],
        ..arduino()
    });
    assert_eq!(picked.parse(line), [("2", 2.0)]);
}

/// Without keys, a series per new name in the order seen, the ones past the cap dropped.
#[test]
fn series_cap() {
    let mut plot = Plot::new("all".to_string(), &PlotDef::default());
    let line: Vec<String> = (0..10).map(|i| format!("k{i}={i}")).collect();
    plot.feed_line(&line.join(" "), secs(1.0));
    plot.feed_line("k9=9 k0=10", secs(2.0));
    let expected: Vec<String> = (0..MAX_SERIES).map(|i| format!("k{i}")).collect();
    assert_eq!(names(plot.data()), expected);
    assert_eq!(points(plot.data(), "k0"), [(1.0, 0.0), (2.0, 10.0)]);
}

/// With keys, their series from the start, in their order, even before a point.
#[test]
fn keys_order_the_series() {
    let mut plot = Plot::new(
        "kv".to_string(),
        &PlotDef {
            keys: vec!["hum".to_string(), "temp".to_string()],
            ..PlotDef::default()
        },
    );
    assert_eq!(names(plot.data()), ["hum", "temp"]);
    plot.feed_line("temp=20 other=1", secs(1.0));
    assert_eq!(names(plot.data()), ["hum", "temp"]);
    assert_eq!(plot.data().series()[0].last(), None);
    assert_eq!(plot.data().series()[1].last(), Some(20.0));

    plot.clear();
    assert_eq!(names(plot.data()), ["hum", "temp"]);
    assert!(plot.data().series().iter().all(|s| s.points().is_empty()));
}

/// A time window keeps the points of its last seconds, each at the time its line arrived:
/// the cut is on the time of the line just fed.
#[test]
fn time_window() {
    let mut plot = Plot::new(
        "kv".to_string(),
        &PlotDef {
            window: PlotWindow::Time(Duration::from_secs(10)),
            ..PlotDef::default()
        },
    );
    plot.feed_line("a=1 b=1", secs(1.0));
    plot.feed_line("a=2", secs(5.0));
    plot.feed_line("no numbers", secs(6.0));
    plot.feed_line("a=3", secs(11.0));
    assert_eq!(
        points(plot.data(), "a"),
        [(1.0, 1.0), (5.0, 2.0), (11.0, 3.0)]
    );
    assert_eq!(points(plot.data(), "b"), [(1.0, 1.0)]);

    plot.feed_line("a=4", secs(11.5));
    assert_eq!(
        points(plot.data(), "a"),
        [(5.0, 2.0), (11.0, 3.0), (11.5, 4.0)]
    );
    // a silent series empties, it stays in the legend
    assert_eq!(points(plot.data(), "b"), []);
    assert_eq!(names(plot.data()), ["a", "b"]);

    let data = plot.data();
    assert_eq!(
        (data.x_start(secs(12.0)), data.x_end(secs(12.0))),
        (2.0, 12.0)
    );
    assert_eq!(data.x_start(secs(3.0)), 0.0);
}

/// A count window keeps the last lines that gave a point, the line's index as x: a series
/// skipped by a line has a gap, and leaves the window as the others go on.
#[test]
fn count_window() {
    let mut plot = Plot::new(
        "arduino".to_string(),
        &PlotDef {
            window: PlotWindow::Samples(3),
            ..arduino()
        },
    );
    plot.feed_line("1 10", Duration::ZERO);
    plot.feed_line("junk", Duration::ZERO);
    plot.feed_line("2", Duration::ZERO);
    plot.feed_line("3", Duration::ZERO);
    assert_eq!(
        points(plot.data(), "1"),
        [(0.0, 1.0), (1.0, 2.0), (2.0, 3.0)]
    );
    assert_eq!(points(plot.data(), "2"), [(0.0, 10.0)]);
    assert_eq!(plot.data().x_end(secs(99.0)), 3.0);

    plot.feed_line("4", Duration::ZERO);
    assert_eq!(
        points(plot.data(), "1"),
        [(1.0, 2.0), (2.0, 3.0), (3.0, 4.0)]
    );
    assert_eq!(points(plot.data(), "2"), []);
    assert_eq!(plot.data().x_start(Duration::ZERO), 1.0);

    plot.clear();
    assert_eq!(names(plot.data()), Vec::<&str>::new());
    plot.feed_line("5", Duration::ZERO);
    assert_eq!(points(plot.data(), "1"), [(0.0, 5.0)]);
}

/// However big the window, a series stops at MAX_POINTS, the oldest going first.
#[test]
fn points_capped() {
    let mut plot = Plot::new(
        "big".to_string(),
        &PlotDef {
            window: PlotWindow::Samples(MAX_POINTS * 2),
            ..arduino()
        },
    );
    for i in 0..MAX_POINTS + 5 {
        plot.feed_line(&i.to_string(), Duration::ZERO);
    }
    let series = &plot.data().series()[0];
    assert_eq!(series.points().len(), MAX_POINTS);
    assert_eq!(series.points().front(), Some(&(5.0, 5.0)));
}

/// The X axis is a whole window wide, ending at the clock on a time window and after the
/// last sample on a count window: a window not full yet fills from the right.
#[test]
fn x_bounds() {
    let mut plot = Plot::new("kv".to_string(), &PlotDef::default());
    assert_eq!(plot.data().x_bounds(secs(5.0)), [-25.0, 5.0]);
    plot.feed_line("a=1", secs(40.0));
    assert_eq!(plot.data().x_bounds(secs(45.0)), [15.0, 45.0]);

    let mut plot = Plot::new("arduino".to_string(), &arduino());
    assert_eq!(plot.data().x_bounds(secs(5.0)), [-500.0, 0.0]);
    plot.feed_line("1", Duration::ZERO);
    plot.feed_line("2", Duration::ZERO);
    assert_eq!(plot.data().x_bounds(secs(99.0)), [-498.0, 2.0]);
}

/// The values from a point on: the last, lowest and highest of them, none without one.
#[test]
fn visible_and_stats() {
    let mut plot = Plot::new("kv".to_string(), &PlotDef::default());
    for (at, value) in [(1.0, 5.0), (2.0, -3.0), (3.0, 8.0), (4.0, 1.0)] {
        plot.feed_line(&format!("a={value}"), secs(at));
    }
    let series = &plot.data().series()[0];
    let visible: Vec<_> = series.visible(2.5).collect();
    assert_eq!(visible, [(3.0, 8.0), (4.0, 1.0)]);
    let all = SeriesStats {
        last: 1.0,
        min: -3.0,
        max: 8.0,
    };
    assert_eq!(series.stats(0.0), Some(all));
    assert_eq!(series.stats(2.0), Some(all));
    assert_eq!(series.stats(2.5).map(|s| (s.min, s.max)), Some((1.0, 8.0)));
    assert_eq!(series.stats(5.0), None);
}

/// Autoscale on the values shown, every series; `min` / `max` over it, a fixed side the
/// values are all past gets a unit range, a flat line a range around it, nothing 0 to 1.
#[test]
fn y_bounds() {
    let mut plot = Plot::new("kv".to_string(), &PlotDef::default());
    let data = |plot: &Plot, min, max| plot.data().y_bounds(0.0, min, max);
    assert_eq!(data(&plot, None, None), [0.0, 1.0]);
    assert_eq!(data(&plot, Some(5.0), None), [5.0, 6.0]);
    assert_eq!(data(&plot, None, Some(5.0)), [4.0, 5.0]);

    plot.feed_line("a=20 b=20", secs(1.0));
    assert_eq!(data(&plot, None, None), [18.0, 22.0]);
    plot.feed_line("a=0", secs(2.0));
    plot.feed_line("b=50", secs(3.0));
    assert_eq!(data(&plot, None, None), [0.0, 50.0]);
    assert_eq!(
        plot.data().y_bounds(2.5, None, None),
        [50.0 - 5.0, 50.0 + 5.0]
    );
    assert_eq!(data(&plot, Some(-10.0), Some(100.0)), [-10.0, 100.0]);
    assert_eq!(data(&plot, Some(10.0), None), [10.0, 50.0]);
    assert_eq!(data(&plot, None, Some(30.0)), [0.0, 30.0]);
    assert_eq!(data(&plot, Some(60.0), None), [60.0, 61.0]);
    assert_eq!(data(&plot, None, Some(-1.0)), [-2.0, -1.0]);

    let mut flat = Plot::new("kv".to_string(), &PlotDef::default());
    flat.feed_line("a=0", secs(1.0));
    assert_eq!(flat.data().y_bounds(0.0, None, None), [-1.0, 1.0]);
}

/// Frozen, the chart shows the series as they were at the time it froze, while the live ones
/// go on; back live, it shows them; cleared, the freeze is over.
#[test]
fn freeze_keeps_what_was_shown() {
    let mut plot = Plot::new("kv".to_string(), &PlotDef::default());
    plot.feed_line("a=1", secs(1.0));
    plot.freeze(Some(secs(2.0)));
    plot.feed_line("a=2 b=5", secs(3.0));

    let (shown, at) = plot.shown(secs(9.0));
    assert_eq!(at, secs(2.0));
    assert_eq!(points(shown, "a"), [(1.0, 1.0)]);
    assert_eq!(shown.series().len(), 1);
    assert_eq!(points(plot.data(), "a"), [(1.0, 1.0), (3.0, 2.0)]);

    plot.freeze(None);
    let (shown, at) = plot.shown(secs(9.0));
    assert_eq!((points(shown, "a").len(), at), (2, secs(9.0)));

    plot.freeze(Some(secs(4.0)));
    plot.clear();
    assert!(!plot.is_frozen());
}

fn fed(chunks: &[&[u8]]) -> Plot {
    let mut plot = Plot::new("kv".to_string(), &PlotDef::default());
    for chunk in chunks {
        plot.feed(chunk, TextEncoding::Utf8, secs(1.0));
    }
    plot
}

fn lasts(plot: &Plot) -> Vec<(&str, Option<f64>)> {
    let series = plot.data().series().iter();
    series.map(|s| (s.name.as_str(), s.last())).collect()
}

/// A line is read at its end of line, cut anywhere between chunks; `\r`, `\n` and `\r\n` all
/// end one, the line still coming in waits.
#[test]
fn lines_from_chunks() {
    let plot = fed(&[b"te", b"mp=2", b"3.4 hum=51\r", b"\nhum=52\npres=10"]);
    assert_eq!(lasts(&plot), [("temp", Some(23.4)), ("hum", Some(52.0))]);
    assert_eq!(points(plot.data(), "hum"), [(1.0, 51.0), (1.0, 52.0)]);

    // a progress line redrawn with `\r`: each draw is a line
    let plot = fed(&[b"\rpct=10\rpct=20\rpct=30"]);
    assert_eq!(points(plot.data(), "pct"), [(1.0, 10.0), (1.0, 20.0)]);
}

/// A line longer than MAX_LINE is skipped whole, up to its end of line; the next one is read.
#[test]
fn long_line_skipped() {
    let long = format!("a=1 {}", "x".repeat(MAX_LINE));
    let plot = fed(&[long.as_bytes(), b" b=2\nc=3\n"]);
    assert_eq!(lasts(&plot), [("c", Some(3.0))]);

    // one byte short of the cap is still read
    let full = format!("a=1 {}", "x".repeat(MAX_LINE - 4));
    assert_eq!(full.len(), MAX_LINE);
    let plot = fed(&[full.as_bytes(), b"\n"]);
    assert_eq!(lasts(&plot), [("a", Some(1.0))]);
}

/// The bytes are decoded with the encoding: a Latin-1 key reads as its letters.
#[test]
fn lines_decoded() {
    let mut plot = Plot::new("kv".to_string(), &PlotDef::default());
    plot.feed(b"temp\xe9=3\n", TextEncoding::Latin1, secs(1.0));
    assert_eq!(lasts(&plot), [("tempé", Some(3.0))]);
}

/// Clearing drops the line still coming in with the points.
#[test]
fn clear_drops_the_pending_line() {
    let mut plot = fed(&[b"a=1\na=2"]);
    plot.clear();
    plot.feed(b"5\n", TextEncoding::Utf8, secs(2.0));
    assert_eq!(lasts(&plot), []);
}

mod app {
    use std::time::Instant;

    use crate::{
        app::{App, tests::toasts},
        configs::{DualBufferMode, KeyBindingAction, Mode, PlotsConfig},
        inputs::{Action, AppAction, DialogAction},
        overlays::Overlay,
    };

    /// An open app with the default plots, `key-value` and `arduino`
    fn app() -> App {
        let mut app = crate::app::tests::app();
        app.set_plots(PlotsConfig::default());
        app
    }

    fn run(app: &mut App, line: &str) {
        app.update(Action::App(AppAction::QueueCommands(
            crate::commands::parse(line).unwrap(),
        )));
        while let Some(action) = app.next_command_action(Instant::now()) {
            app.update(action);
        }
    }

    fn shown(app: &App) -> Option<&str> {
        app.plot().map(|plot| plot.name.as_str())
    }

    fn last_of(app: &App, name: &str) -> Option<f64> {
        let series = app.plot()?.data().series().iter();
        series.filter(|s| s.name == name).find_map(|s| s.last())
    }

    fn last_toast(app: &App) -> String {
        toasts(app).pop().unwrap()
    }

    /// MAX-222: off at start, no parsing; once shown, the plot reads live RX in both modes,
    /// whichever mode is fed (`active_only`), and a replay's bytes too.
    /// How: a line fed with no plot, then each mode with both buffering settings, then a
    /// replay link.
    #[test]
    fn plot_reads_rx() {
        let mut app = app();
        app.feed_rx(b"temp=1\n");
        assert!(app.plot().is_none());

        run(&mut app, "plot key-value");
        let mut value = 0.0;
        for mode in [Mode::Line, Mode::Terminal] {
            for buffering in [DualBufferMode::AlwaysBoth, DualBufferMode::ActiveOnly] {
                app.settings_mut().mode = mode;
                app.settings_mut().dual_mode_buffering = buffering;
                value += 1.0;
                app.feed_rx(format!("temp={value}").as_bytes());
                app.feed_rx(b"\r\n");
                assert_eq!(last_of(&app, "temp"), Some(value), "{mode:?} {buffering:?}");
            }
        }

        app.link_mut()
            .set_address("file:///tmp/capture.raw".parse().unwrap());
        app.feed_rx(b"temp=9\n");
        assert_eq!(last_of(&app, "temp"), Some(9.0));
    }

    /// MAX-222: `plot <name>` shows that plot, `plot off` gives the side back, `plot` alone
    /// goes to the next one in the file's order and off after the last; the plot shown again
    /// keeps its points. A name that isn't a plot, and any `plot` with no plot in the file,
    /// are refused. The command line completes the names and `off`.
    /// How: the default plots, then none.
    #[test]
    fn plot_command() {
        let mut app = app();
        run(&mut app, "plot arduino");
        assert_eq!(shown(&app), Some("arduino"));
        assert_eq!(last_toast(&app), "Plot arduino");
        app.feed_rx(b"1 2\n");
        run(&mut app, "plot arduino");
        assert_eq!(last_of(&app, "2"), Some(2.0));

        run(&mut app, "plot off");
        assert_eq!(shown(&app), None);
        assert_eq!(last_toast(&app), "Plot off");

        run(&mut app, "plot");
        assert_eq!(shown(&app), Some("key-value"));
        run(&mut app, "plot");
        assert_eq!(shown(&app), Some("arduino"));
        assert_eq!(last_of(&app, "2"), None);
        run(&mut app, "plot; plot");
        assert_eq!(shown(&app), Some("tagged"));
        run(&mut app, "plot");
        assert_eq!(shown(&app), None);

        run(&mut app, "plot arduino; plot nope; plot off");
        assert_eq!(
            last_toast(&app),
            "plot: no plot named 'nope' (1 more not run)"
        );
        assert_eq!(shown(&app), Some("arduino"));

        app.update(KeyBindingAction::CommandLine.to_action(&app));
        let complete = |app: &mut App, line: &str| {
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            let Some(Overlay::CommandLine(command_line)) = app.overlay() else {
                panic!("the command line");
            };
            let candidates = command_line.completion.candidates.iter();
            candidates.map(|c| c.value.clone()).collect::<Vec<_>>()
        };
        assert_eq!(
            complete(&mut app, "plot "),
            ["key-value", "arduino", "sensors", "tagged", "off"]
        );
        app.update(Action::Overlay(DialogAction::Close));

        app.set_plots(PlotsConfig { plots: Vec::new() });
        run(&mut app, "plot");
        assert_eq!(last_toast(&app), "plot: plots.toml has no plot");
    }

    /// MAX-222: the plot starts over with the views, still shown: a replay started, and the
    /// live link back after it, as for a device switched to (`clear_screen`).
    /// How: a point, a replay, a point, the replay stopped.
    #[test]
    fn plot_cleared_with_the_views() {
        let mut app = app();
        run(&mut app, "plot key-value");
        app.feed_rx(b"a=1\nb=");
        app.start_replay("/tmp/x.raw".into(), None);
        assert_eq!(shown(&app), Some("key-value"));
        assert_eq!(last_of(&app, "a"), None);
        // the half line before doesn't complete with the replay's bytes
        app.feed_rx(b"2\n");
        assert_eq!(last_of(&app, "b"), None);
        app.feed_rx(b"a=3\n");
        assert!(app.stop_replay());
        assert_eq!(shown(&app), Some("key-value"));
        assert_eq!(last_of(&app, "a"), None);
    }

    /// MAX-222: a reload keeps the plot shown and its points while its definition is the
    /// same, rebuilds it on fresh series when it changed, and turns it off, said, when it is
    /// gone from the file.
    /// How: the default plots again, then `key-value` with keys, then without it.
    #[test]
    fn plot_on_reload() {
        let mut app = app();
        run(&mut app, "plot key-value");
        app.feed_rx(b"a=1 b=2\n");

        app.set_plots(PlotsConfig::default());
        assert_eq!(last_of(&app, "a"), Some(1.0));

        let mut plots = PlotsConfig::default();
        plots.plots[0].1.keys = vec!["b".to_string()];
        app.set_plots(plots.clone());
        assert_eq!(last_of(&app, "a"), None);
        app.feed_rx(b"a=1 b=2\n");
        assert_eq!(last_of(&app, "b"), Some(2.0));
        assert_eq!(last_of(&app, "a"), None);

        plots.plots.remove(0);
        app.set_plots(plots);
        assert_eq!(shown(&app), None);
        assert_eq!(
            last_toast(&app),
            "Plot key-value is gone from plots.toml: plot off"
        );
    }

    /// MAX-222: `plot-freeze` (`Ctrl-a G`) freezes the plot shown and gives it back live,
    /// each said; with no plot shown it says so. A device switch, as a replay's start, ends
    /// the freeze. `Ctrl-a g` goes to the next plot as `plot` alone does; `plot-next` and
    /// `plot-prev` step both ways, off past the ends. The settings popup's `V` goes to the
    /// next one; its `g` is the popups' Home, not a plot key.
    /// How: the commands, then the default keys, then the popup's keys typed.
    #[test]
    fn plot_freeze_and_keys() {
        let mut app = app();
        run(&mut app, "plot-freeze");
        assert_eq!(last_toast(&app), "No plot shown");

        run(&mut app, "plot key-value; plot-freeze");
        assert!(app.plot().unwrap().is_frozen());
        assert_eq!(last_toast(&app), "Plot key-value frozen");
        app.feed_rx(b"a=1\n");
        assert_eq!(last_of(&app, "a"), Some(1.0));
        let (frozen, _) = app.plot().unwrap().shown(app.plot_clock());
        assert!(frozen.series().is_empty());
        run(&mut app, "plot-freeze");
        assert!(!app.plot().unwrap().is_frozen());
        assert_eq!(last_toast(&app), "Plot key-value live");

        run(&mut app, "plot-freeze");
        app.start_replay("/tmp/x.raw".into(), None);
        assert!(!app.plot().unwrap().is_frozen());

        let prefix_key = |app: &mut App, c: char| {
            app.update(Action::App(AppAction::SetPrefix(true)));
            let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            ));
            let action = crate::inputs::handle_event(event, app);
            app.update(action);
            while let Some(action) = app.next_command_action(Instant::now()) {
                app.update(action);
            }
        };
        prefix_key(&mut app, 'G');
        assert!(app.plot().unwrap().is_frozen());
        prefix_key(&mut app, 'g');
        assert_eq!(shown(&app), Some("arduino"));

        run(&mut app, "plot-prev");
        assert_eq!(shown(&app), Some("key-value"));
        run(&mut app, "plot-prev");
        assert_eq!(shown(&app), None);
        run(&mut app, "plot-prev");
        assert_eq!(shown(&app), Some("tagged"));
        run(&mut app, "plot-next");
        assert_eq!(shown(&app), None);
        run(&mut app, "plot-next");
        assert_eq!(shown(&app), Some("key-value"));

        let typed = |app: &mut App, c: char| {
            let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            ));
            let action = crate::inputs::handle_event(event, app);
            app.update(action);
        };
        app.update(KeyBindingAction::Settings.to_action(&app));
        typed(&mut app, 'g');
        assert_eq!(shown(&app), Some("key-value"));
        typed(&mut app, 'v');
        assert_eq!(shown(&app), Some("arduino"));
        typed(&mut app, 'V');
        assert_eq!(shown(&app), Some("sensors"));
        assert!(matches!(app.overlay(), Some(Overlay::Settings(_))));

        app.set_plots(PlotsConfig { plots: Vec::new() });
        run(&mut app, "plot-next");
        assert_eq!(last_toast(&app), "plots.toml has no plot");
    }

    /// MAX-222: the side resize keys warn that there's no side panel only when neither the
    /// raw view nor a plot shows one.
    #[test]
    fn resize_side_with_a_plot() {
        let mut app = app();
        let resize = |app: &mut App| app.update(Action::App(AppAction::ResizeSidePanel(5)));
        resize(&mut app);
        assert_eq!(last_toast(&app), "SideView is Off");

        run(&mut app, "plot key-value");
        let before = toasts(&app).len();
        let percent = app.settings().raw_side.split_percent;
        resize(&mut app);
        assert_eq!(toasts(&app).len(), before);
        assert_eq!(app.settings().raw_side.split_percent, percent + 5);
    }

    /// MAX-222: `plot` in config.toml (or `--plot` over it) shows that plot at start; a name
    /// that isn't a plot is said and the plot stays off.
    #[test]
    fn plot_at_start() {
        let mut app = app();
        app.show_start_plot();
        assert_eq!(shown(&app), None);

        app.settings_mut().plot = Some("arduino".to_string());
        app.show_start_plot();
        assert_eq!(shown(&app), Some("arduino"));

        let mut app = self::app();
        app.settings_mut().plot = Some("nope".to_string());
        app.show_start_plot();
        assert_eq!(shown(&app), None);
        assert_eq!(
            last_toast(&app),
            "No plot named 'nope' in plots.toml: plot off"
        );
    }
}
