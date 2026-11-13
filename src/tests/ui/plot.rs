use std::time::Duration;

use super::*;
use crate::configs::plots::PlotDef;

fn plot_with(def: PlotDef, lines: &[&str]) -> Plot {
    let mut plot = Plot::new("p".to_string(), &def);
    for line in lines {
        plot.feed_line(line, Duration::from_secs(1));
    }
    plot
}

#[test]
fn value_labels() {
    let cases = [
        (0.0, "0"),
        (-0.0, "0"),
        (23.4, "23.4"),
        (25.0, "25"),
        (-1.5, "-1.5"),
        (0.12345, "0.123"),
        (1013.25, "1013.25"),
        (999_999.0, "999999"),
        (1_500_000.0, "1.50e6"),
        (0.0004, "4.00e-4"),
    ];
    for (value, label) in cases {
        assert_eq!(value_label(value), label, "{value}");
    }
}

/// A text cut to its width with `…`, wide characters counted as two columns.
#[test]
fn shortened() {
    assert_eq!(shorten("pid", 10), "pid");
    assert_eq!(shorten("0123456789", 10), "0123456789");
    assert_eq!(shorten("0123456789a", 10), "012345678…");
    assert_eq!(shorten("温度温度温度", 6), "温度…");
}

/// The name, last, lowest and highest value shown; the name alone without a point.
#[test]
fn legend_entries() {
    let plot = plot_with(PlotDef::default(), &["t=20 h=1", "t=25", "t=22.5"]);
    let series = plot.data().series();
    assert_eq!(legend_entry(&series[0], 0.0), "t 22.5 ↓20 ↑25");
    assert_eq!(legend_entry(&series[1], 0.0), "h 1 ↓1 ↑1");
    assert_eq!(legend_entry(&series[1], 5.0), "h");
}

/// A series' own color, else the next link color: the ones with their own don't use one up.
#[test]
fn colors_with_the_link_fallback() {
    let def = PlotDef {
        colors: [("b".to_string(), Color::Red)].into(),
        ..PlotDef::default()
    };
    let plot = plot_with(def, &["a=1 b=2 c=3"]);
    let links = ThemeConfig::default().palette.links;
    assert_eq!(
        series_colors(&plot, &ThemeConfig::default()),
        [links[0], Color::Red, links[1]]
    );
}

/// The hint says the format the plot reads.
#[test]
fn waiting_hints() {
    let plot = plot_with(PlotDef::default(), &[]);
    assert_eq!(waiting_hint(&plot), "Waiting for key=value");
    let def = PlotDef {
        separator: ":".to_string(),
        columns: true,
        line_contains: Some("PID".to_string()),
        ..PlotDef::default()
    };
    assert_eq!(
        waiting_hint(&plot_with(def, &[])),
        "Waiting for key:value or bare numbers in lines with 'PID'"
    );
}

/// MAX-230: about one Y label every 4 rows, 3 to 5 of them, 2 on a very short chart.
#[test]
fn y_label_counts() {
    let counts: Vec<usize> = [0, 3, 4, 8, 11, 12, 15, 16, 40]
        .into_iter()
        .map(y_label_count)
        .collect();
    assert_eq!(counts, [2, 2, 3, 3, 3, 4, 4, 5, 5]);
}

/// MAX-230: the Y labels sit on round values inside the axis' bounds, at most as many as
/// asked: bounds already on a step are labels, below 1 and negative values too, and a flat
/// or broken range gives its two ends.
#[test]
fn y_tick_values() {
    let cases: [([f64; 2], usize, &[f64]); 8] = [
        ([0.0, 52.5], 3, &[0.0, 20.0, 40.0]),
        ([0.0, 52.5], 5, &[0.0, 20.0, 40.0]),
        ([0.0, 1.0], 3, &[0.0, 0.5, 1.0]),
        ([0.0, 100.0], 5, &[0.0, 25.0, 50.0, 75.0, 100.0]),
        ([14.45, 25.55], 3, &[15.0, 20.0, 25.0]),
        ([-11.0, 11.0], 3, &[-10.0, 0.0, 10.0]),
        ([0.0, 0.042], 4, &[0.0, 0.02, 0.04]),
        ([1012.97, 1013.63], 4, &[1013.0, 1013.2, 1013.4, 1013.6]),
    ];
    for (bounds, count, want) in cases {
        let got = y_ticks(bounds, count);
        assert!(got.len() <= count, "{bounds:?} {count}: {got:?}");
        let close =
            got.len() == want.len() && got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9);
        assert!(close, "{bounds:?} {count}: {got:?}, not {want:?}");
    }
    assert_eq!(y_ticks([5.0, 5.0], 3), [5.0, 5.0]);
}

/// MAX-230: a value's row: the low bound on the graph's bottom row, the high one on its
/// top row, in between rounded to the nearest row, past the bounds kept on the graph.
#[test]
fn value_rows() {
    let graph = Rect::new(4, 10, 20, 9);
    let rows: Vec<u16> = [0.0, 50.0, 100.0, 12.5, 26.0, -5.0, 120.0]
        .into_iter()
        .map(|value| value_row(value, [0.0, 100.0], graph))
        .collect();
    assert_eq!(rows, [18, 14, 10, 17, 16, 18, 10]);
}

/// MAX-230: the compact legend in the top left corner of the curves' room: a blank row as
/// wide as the first entry, then a row per entry, each a blank column, the entry and a
/// space wide; none when it doesn't fit (the widest row, or a row per entry and the blank
/// one), or with no series.
#[test]
fn legend_rows_layout() {
    let graph = Rect::new(5, 3, 20, 5);
    let entries = |names: &[&str]| -> Vec<(String, Color)> {
        names
            .iter()
            .map(|name| (name.to_string(), Color::Red))
            .collect()
    };
    assert_eq!(
        legend_rows(graph, &entries(&["temp 20", "hum 51 ↓40"])),
        Some(vec![
            Rect::new(5, 3, 9, 1),
            Rect::new(5, 4, 9, 1),
            Rect::new(5, 5, 12, 1),
        ])
    );
    assert!(legend_rows(graph, &entries(&["x".repeat(18).as_str()])).is_some());
    assert!(legend_rows(graph, &entries(&["x".repeat(19).as_str()])).is_none());
    assert!(legend_rows(graph, &entries(&["a", "b", "c", "d"])).is_some());
    assert!(legend_rows(graph, &entries(&["a", "b", "c", "d", "e"])).is_none());
    assert_eq!(legend_rows(graph, &[]), None);
}

/// MAX-230: the X labels: the window, `samples` after a count when it fits in its share of
/// the width, the window's middle from 40 columns on, then `now`.
#[test]
fn x_label_sets() {
    use std::time::Duration;
    let time = PlotWindow::Time(Duration::from_secs(30));
    let samples = PlotWindow::Samples(500);
    let cases: [(PlotWindow, u16, &[&str]); 6] = [
        (time, 90, &["-30s", "-15s", "now"]),
        (time, 39, &["-30s", "now"]),
        (
            PlotWindow::Time(Duration::from_secs(300)),
            60,
            &["-5m", "-2m 30s", "now"],
        ),
        (samples, 90, &["-500 samples", "-250", "now"]),
        (samples, 30, &["-500 samples", "now"]),
        (samples, 20, &["-500", "now"]),
    ];
    for (window, width, want) in cases {
        assert_eq!(x_labels(window, width), want, "{window:?} {width}");
    }
}
