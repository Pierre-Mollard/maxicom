//! The plot panel (MAX-222): the plot shown, in the side panel, placed by `[plot_side]` (MAX-230)

use humantime_serde::re::humantime;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Clear, Dataset, GraphType, Paragraph},
};

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{
        App,
        plots::{Plot, PlotSeries},
        types::{ClickAction, ClickTarget},
    },
    configs::{KeyBindingAction, ThemeConfig, plots::PlotWindow, settings::RawSideOrientation},
};

/// The title row's columns besides the name: `  ◀ ` and ` ▶`, a space at least, `✕ `
const TITLE_FIXED_WIDTH: usize = 4 + 2 + 1 + 2;

/// The plot shown in `area`, its border a plain separator from the rest (MAX-230), then on
/// its first row the name between `◀` and `▶`, which a click runs `plot-prev` / `plot-next`
/// with, `frozen` after them while it is, and `✕` at its right end, which closes it: a line per series, the X axis
/// its window back from now, the Y axis its `min` / `max` or the values shown, a legend in the
/// top left with each series' last, lowest and highest value shown. The buttons' click
/// targets are given back
pub fn render_plot_panel(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &ThemeConfig,
) -> Vec<ClickTarget> {
    let Some(plot) = app.plot() else {
        return Vec::new();
    };
    let borders = match app.settings().plot_side.orientation {
        RawSideOrientation::Right => Borders::LEFT,
        RawSideOrientation::Bottom => Borders::TOP,
    };
    let block = Block::default()
        .borders(borders)
        .border_style(*theme.ui.border_panel);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return Vec::new();
    }

    // `  ◀ name ▶ frozen` under the separator, in from the border, and `✕` at the row's
    // right end: the name cut to what's left
    let text = Style::default().fg(theme.palette.text);
    let muted = Style::default().fg(theme.palette.text_muted);
    let button_style = Style::default().fg(theme.palette.secondary);
    let frozen = if plot.is_frozen() { " frozen" } else { "" };
    let room = (inner.width as usize).saturating_sub(TITLE_FIXED_WIDTH + frozen.len());
    let name = shorten(&plot.name, room.max(3));
    let name_width = name.width() as u16;
    let title_row = Rect { height: 1, ..inner };
    let title = Line::from(vec![
        Span::raw("  "),
        Span::styled("◀", button_style),
        Span::styled(format!(" {name} "), text),
        Span::styled("▶", button_style),
        Span::styled(frozen, Style::default().fg(theme.palette.text_warn)),
    ]);
    frame.render_widget(Paragraph::new(title), title_row);
    let close_x = inner.width.saturating_sub(2);
    let close = Rect::new(inner.x + close_x, inner.y, 1, 1).intersection(title_row);
    frame.render_widget(Paragraph::new("✕").style(button_style), close);
    let button = |at: u16, action| ClickTarget {
        area: Rect::new(inner.x + at, inner.y, 1, 1).intersection(title_row),
        action,
        hover: Some(*theme.ui.hover),
    };
    let targets = vec![
        button(2, ClickAction::Run(KeyBindingAction::PlotPrev)),
        button(5 + name_width, ClickAction::Run(KeyBindingAction::PlotNext)),
        button(close_x, ClickAction::ClosePlot),
    ];
    // a column of padding each side: the newest points and `now` off the app's edge; the right
    // one widened below to match the Y labels' side
    let chart_area = Rect::new(
        inner.x + 1,
        inner.y + 1,
        inner.width.saturating_sub(2),
        inner.height - 1,
    );

    let (data, at) = plot.shown(app.plot_clock());
    let def = plot.def();
    let x_bounds = data.x_bounds(at);
    let from = x_bounds[0];
    let y_bounds = data.y_bounds(from, def.min, def.max);
    // round values, as many as the height takes, on their own rows (drawn after the chart)
    let ticks = y_ticks(y_bounds, y_label_count(chart_area.height.saturating_sub(2)));
    let tick_labels: Vec<String> = ticks.iter().copied().map(value_label).collect();
    let label_width = tick_labels
        .iter()
        .map(|label| label.width())
        .max()
        .unwrap_or(1);
    let points: Vec<Vec<(f64, f64)>> = (data.series().iter())
        .map(|series| series.visible(from).collect())
        .collect();
    let colors = series_colors(plot, theme);
    let datasets = (data.series().iter())
        .zip(&colors)
        .zip(&points)
        .map(|((_, color), points)| {
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(*color))
                .data(points)
        })
        .collect();

    // blank labels: ratatui keeps their column and draws the axis, ours go in it
    let y_labels = vec![" ".repeat(label_width); 2];
    // the curves centered: as much room right of them as the labels take left of the axis
    let labels_width = graph_area(chart_area, &y_labels).x - chart_area.x;
    let chart_area = Rect {
        width: chart_area
            .width
            .saturating_sub(labels_width.saturating_sub(1)),
        ..chart_area
    };
    let x_labels = x_labels(def.window, graph_area(chart_area, &y_labels).width);
    let chart = Chart::new(datasets)
        .x_axis(
            Axis::default()
                .bounds(x_bounds)
                .labels(x_labels)
                // the first label starts at the Y axis, not under the Y labels (MAX-230)
                .labels_alignment(Alignment::Right)
                .style(muted),
        )
        .y_axis(
            Axis::default()
                .bounds(y_bounds)
                .labels(y_labels.clone())
                .style(muted),
        )
        // drawn below: ratatui's doesn't clear the curves under it
        .legend_position(None);
    frame.render_widget(chart, chart_area);

    // each tick right aligned against the Y axis, on the row its value falls on
    let graph = graph_area(chart_area, &y_labels);
    if graph.height > 0 && graph.x > chart_area.x {
        let axis = graph.x - 1;
        let column_x = axis.saturating_sub(label_width as u16).max(chart_area.x);
        let mut last_row = None;
        for (value, label) in ticks.iter().zip(&tick_labels) {
            let row = value_row(*value, y_bounds, graph);
            if last_row == Some(row) {
                continue;
            }
            last_row = Some(row);
            let cell = Rect::new(column_x, row, axis - column_x, 1);
            let label = Paragraph::new(label.as_str())
                .style(muted)
                .alignment(Alignment::Right);
            frame.render_widget(label, cell);
        }
    }

    // the legend: top left, over the oldest points (the newest come in on the right)
    let entries: Vec<(String, Color)> = (data.series().iter())
        .map(|series| legend_entry(series, from))
        .zip(colors)
        .collect();
    if let Some(rows) = legend_rows(graph, &entries) {
        // each row cleared as wide as its text and the gaps, the curves visible around it
        for row in &rows {
            frame.render_widget(Clear, *row);
            frame.render_widget(Block::default().style(*theme.ui.background), *row);
        }
        for (row, (entry, color)) in rows.iter().skip(1).zip(&entries) {
            let text = Rect::new(row.x + 1, row.y, row.width - 2, 1);
            let entry = Paragraph::new(entry.as_str()).style(Style::default().fg(*color));
            frame.render_widget(entry, text);
        }
    }

    // in the middle of the curves' room, not of the panel with its axes
    if points.iter().all(Vec::is_empty) && graph.height > 0 {
        let row = Rect {
            y: graph.y + graph.height / 2,
            height: 1,
            ..graph
        };
        let hint = Paragraph::new(waiting_hint(plot))
            .style(muted)
            .alignment(Alignment::Center);
        frame.render_widget(hint, row);
    }
    targets
        .into_iter()
        .filter(|target| !target.area.is_empty())
        .collect()
}

/// Where ratatui's `Chart` draws the curves in `area`: right of the Y labels and the Y axis,
/// above the X axis and its labels, its first one right aligned so it doesn't widen the Y
/// labels' column (`ratatui-widgets` 0.3, `Chart::layout`)
pub fn graph_area(area: Rect, y_labels: &[String]) -> Rect {
    if area.height < 3 || area.width < 2 {
        return Rect { height: 0, ..area };
    }
    let y_width = y_labels
        .iter()
        .map(|label| label.width())
        .max()
        .unwrap_or(0) as u16;
    let left = y_width.min(area.width / 3);
    let x = area.x + left + 1;
    Rect::new(x, area.y, area.right().saturating_sub(x), area.height - 2)
}

/// The Y labels for a chart `rows` high: about one every 4 rows, 3 to 5 of them, 2 on a
/// chart too short for 3
pub fn y_label_count(rows: u16) -> usize {
    match rows {
        0..=3 => 2,
        4..=11 => 3,
        12..=15 => 4,
        _ => 5,
    }
}

/// The Y labels' values: the multiples of the smallest round step (see `round_step`) inside
/// `bounds` that fit in `count`, so `[0, 52.5]` with 3 gives 0, 20, 40
pub fn y_ticks(bounds: [f64; 2], count: usize) -> Vec<f64> {
    let [lo, hi] = bounds;
    if !(lo.is_finite() && hi.is_finite()) || hi <= lo || count < 2 {
        return vec![lo, hi];
    }
    // the epsilons keep a bound already on a step, through float noise
    let span = |step: f64| ((lo / step - 1e-9).ceil(), (hi / step + 1e-9).floor());
    let mut step = round_step((hi - lo) / count as f64);
    let (mut first, mut last) = span(step);
    while last - first + 1.0 > count as f64 {
        step = round_step(step * 1.001);
        (first, last) = span(step);
    }
    let ticks = (first as i64..=last as i64).map(|n| n as f64 * step);
    // `-0` reads `0`
    ticks
        .map(|tick| if tick == 0.0 { 0.0 } else { tick })
        .collect()
}

/// The screen row of `value` on a Y axis from `bounds[0]` at the graph's bottom row to
/// `bounds[1]` at its top
pub fn value_row(value: f64, bounds: [f64; 2], graph: Rect) -> u16 {
    let [lo, hi] = bounds;
    let share = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let up = (share * f64::from(graph.height.saturating_sub(1))).round() as u16;
    graph.bottom().saturating_sub(1) - up
}

/// The smallest round step at least `raw`: 1, 2, 2.5 or 5 times a power of ten
fn round_step(raw: f64) -> f64 {
    let magnitude = 10f64.powf(raw.log10().floor());
    let mantissa = raw / magnitude;
    let round = [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .find(|round| mantissa <= round * (1.0 + 1e-9))
        .unwrap_or(10.0);
    round * magnitude
}

/// The compact legend's rows in the top left corner of `graph` (MAX-230): a blank row, then
/// a row per entry, each a blank column, the entry and a space wide, the blank row as wide as
/// the first entry's; none when they don't fit, or with no series
pub fn legend_rows(graph: Rect, entries: &[(String, Color)]) -> Option<Vec<Rect>> {
    let widths: Vec<u16> = (entries.iter())
        .map(|(entry, _)| entry.width() as u16 + 2)
        .collect();
    let widest = *widths.iter().max()?;
    let height = entries.len() as u16 + 1;
    if widest <= 2 || widest > graph.width || height > graph.height {
        return None;
    }
    let row = |y: u16, width: u16| Rect::new(graph.x, graph.y + y, width, 1);
    let gap = row(0, widths[0]);
    Some(
        std::iter::once(gap)
            .chain((1..).zip(widths).map(|(y, width)| row(y, width)))
            .collect(),
    )
}

/// `text` in `width` columns at most, cut with `…` when it's longer
pub fn shorten(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Each series' color: its own from `colors`, else the next of the theme's link colors
pub fn series_colors(plot: &Plot, theme: &ThemeConfig) -> Vec<Color> {
    let mut links = theme.palette.links.iter().cycle();
    (plot.data().series().iter())
        .map(|series| match plot.def().colors.get(&series.name) {
            Some(color) => *color,
            None => *links.next().expect("a cycle of 8 colors"),
        })
        .collect()
}

/// `temp 23.4 ↓12.1 ↑25`: the name, the last value shown, the lowest and the highest; the
/// name alone while none is shown
pub fn legend_entry(series: &PlotSeries, from: f64) -> String {
    match series.stats(from) {
        Some(stats) => format!(
            "{} {} ↓{} ↑{}",
            series.name,
            value_label(stats.last),
            value_label(stats.min),
            value_label(stats.max)
        ),
        None => series.name.clone(),
    }
}

/// The left end of the X axis: the window back from now
fn window_label(window: PlotWindow) -> String {
    match window {
        PlotWindow::Time(duration) => format!("-{}", humantime::format_duration(duration)),
        PlotWindow::Samples(count) => format!("-{count}"),
    }
}

/// The X axis' labels for a chart `width` columns wide (MAX-230): the window back from now,
/// `samples` after a count when it fits in its third, the middle of the window when the
/// chart is 40 columns or more, then `now`
pub fn x_labels(window: PlotWindow, width: u16) -> Vec<String> {
    let middle = (width >= 40).then(|| match window {
        PlotWindow::Time(duration) => window_label(PlotWindow::Time(duration / 2)),
        PlotWindow::Samples(count) => window_label(PlotWindow::Samples(count / 2)),
    });
    let room = usize::from(width) / if middle.is_some() { 3 } else { 2 };
    let start = match window {
        PlotWindow::Samples(count) if format!("-{count} samples").width() < room => {
            format!("-{count} samples")
        }
        _ => window_label(window),
    };
    std::iter::once(start)
        .chain(middle)
        .chain(std::iter::once("now".to_string()))
        .collect()
}

/// A value short enough for an axis: 3 decimals at most, trailing zeros dropped, and an
/// exponent for the very large or very small
pub fn value_label(value: f64) -> String {
    let size = value.abs();
    if value == 0.0 {
        "0".to_string()
    } else if !(1e-3..1e6).contains(&size) {
        format!("{value:.2e}")
    } else {
        let text = format!("{value:.3}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// What the panel says before a number came in: the format it reads
fn waiting_hint(plot: &Plot) -> String {
    let def = plot.def();
    let format = if def.columns {
        format!("key{}value or bare numbers", def.separator)
    } else {
        format!("key{}value", def.separator)
    };
    match &def.line_contains {
        Some(text) => format!("Waiting for {format} in lines with '{text}'"),
        None => format!("Waiting for {format}"),
    }
}

#[cfg(test)]
#[path = "../tests/ui/plot.rs"]
mod tests;
