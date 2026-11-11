//! The plot panel (MAX-222): the plot shown, in the raw side panel's place

use humantime_serde::re::humantime;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Rect},
    style::{Color, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, LegendPosition, Paragraph},
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

/// The plot shown in `area`, with the raw panel's border and as title its name between `◀`
/// and `▶`, which a click runs `plot-prev` / `plot-next` with, `frozen` after it while it is:
/// a line per series, the X axis its window back from now, the Y axis its `min` / `max` or
/// the values shown, a legend in the top left with each series' last, lowest and highest
/// value shown. The arrows' click targets are given back
pub fn render_plot_panel(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &ThemeConfig,
) -> Vec<ClickTarget> {
    let Some(plot) = app.plot() else {
        return Vec::new();
    };
    let (borders, title_x) = match app.settings().raw_side.orientation {
        RawSideOrientation::Right => (Borders::LEFT, area.x + 1),
        RawSideOrientation::Bottom => (Borders::TOP, area.x),
    };
    let text = Style::default().fg(theme.palette.text);
    let muted = Style::default().fg(theme.palette.text_muted);
    let arrow = Style::default().fg(theme.palette.secondary);
    // ` ◀ name ▶ frozen `: the name cut to what's left
    let frozen = if plot.is_frozen() { "frozen " } else { "" };
    let room = (area.right().saturating_sub(title_x) as usize).saturating_sub(6 + frozen.len());
    let name = shorten(&plot.name, room.max(3));
    let name_width = name.width() as u16;
    let block = Block::default()
        .borders(borders)
        .border_style(*theme.ui.border_panel)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("◀", arrow),
            Span::styled(format!(" {name} "), text),
            Span::styled("▶", arrow),
            Span::raw(" "),
            Span::styled(frozen, Style::default().fg(theme.palette.text_warn)),
        ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let button = |x: u16, action| ClickTarget {
        area: Rect::new(x, area.y, 1, 1).intersection(area),
        action: ClickAction::Run(action),
        hover: Some(*theme.ui.hover),
    };
    let targets = vec![
        button(title_x + 1, KeyBindingAction::PlotPrev),
        button(title_x + 4 + name_width, KeyBindingAction::PlotNext),
    ];

    let (data, at) = plot.shown(app.plot_clock());
    let def = plot.def();
    let x_bounds = data.x_bounds(at);
    let from = x_bounds[0];
    let y_bounds = data.y_bounds(from, def.min, def.max);
    let points: Vec<Vec<(f64, f64)>> = (data.series().iter())
        .map(|series| series.visible(from).collect())
        .collect();
    let datasets = (data.series().iter())
        .zip(series_colors(plot, theme))
        .zip(&points)
        .map(|((series, color), points)| {
            Dataset::default()
                .name(legend_entry(series, from))
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(color))
                .data(points)
        })
        .collect();

    let x_labels = [window_label(def.window), "now".to_string()];
    let mid = (y_bounds[0] + y_bounds[1]) / 2.0;
    let y_labels = [y_bounds[0], mid, y_bounds[1]].map(value_label);
    let chart = Chart::new(datasets)
        .x_axis(
            Axis::default()
                .bounds(x_bounds)
                .labels(x_labels)
                .style(muted),
        )
        .y_axis(
            Axis::default()
                .bounds(y_bounds)
                .labels(y_labels)
                .style(muted),
        )
        // top left: the oldest points, the newest come in on the right
        .legend_position(Some(LegendPosition::TopLeft))
        .hidden_legend_constraints((Constraint::Percentage(100), Constraint::Percentage(100)));
    frame.render_widget(chart, inner);

    if points.iter().all(Vec::is_empty) && inner.height > 0 {
        let row = Rect {
            y: inner.y + inner.height / 2,
            height: 1,
            ..inner
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
