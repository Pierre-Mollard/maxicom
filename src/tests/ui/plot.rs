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
