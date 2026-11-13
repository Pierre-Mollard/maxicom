//! Live plotter (MAX-222): the numbers of a plot read in RX lines, and the series they feed
//!
//! `Plot` is pure: RX bytes and the time they arrived come in, points go out. The app picks
//! the plot, feeds it live RX (a replay's too) with its clock, and clears it with the views.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::{
    app::App,
    configs::{
        PlotsConfig, TextEncoding,
        plots::{MAX_SERIES, PlotDef, PlotWindow},
    },
    notification::ToastLevel,
};

/// The most points a series keeps, whatever its window: a long time window on a fast device
/// or a huge count stops there
pub const MAX_POINTS: usize = 50_000;

/// The share of the values' range an autoscaled Y axis adds past them (MAX-230)
const HEADROOM: f64 = 0.05;

/// The longest line read: the rest of a longer one is skipped up to its end of line
pub const MAX_LINE: usize = 4096;

/// A bare column's series name, by its place in the line
const COLUMN_NAMES: [&str; MAX_SERIES] = ["1", "2", "3", "4", "5", "6", "7", "8"];

/// A plot: its definition, what it reads in a line, the series it fed, and the line still
/// coming in. Frozen, it keeps the series as they were and the time it froze at, for the
/// chart, while the live ones go on
#[derive(Debug, Clone)]
pub struct Plot {
    pub name: String,
    def: PlotDef,
    parser: PlotParser,
    data: PlotData,
    pending: Vec<u8>,
    /// The line coming in is longer than `MAX_LINE`: skipped up to its end of line
    skipping: bool,
    frozen: Option<(PlotData, Duration)>,
}

impl Plot {
    pub fn new(name: String, def: &PlotDef) -> Self {
        Self {
            name,
            def: def.clone(),
            parser: PlotParser::new(def),
            data: PlotData::new(def),
            pending: Vec::new(),
            skipping: false,
            frozen: None,
        }
    }

    pub fn def(&self) -> &PlotDef {
        &self.def
    }

    /// RX bytes arrived at `at`: each line they finish is read, the rest waits for its end.
    /// `\r` and `\n` both end a line, so `\r\n` and a line redrawn with `\r` are read too
    pub fn feed(&mut self, chunk: &[u8], encoding: TextEncoding, at: Duration) {
        let mut rest = chunk;
        while let Some(end) = rest.iter().position(|b| matches!(b, b'\r' | b'\n')) {
            self.take(&rest[..end]);
            if !std::mem::take(&mut self.skipping) && !self.pending.is_empty() {
                let line = encoding.decode_cow(&self.pending).into_owned();
                self.feed_line(&line, at);
            }
            self.pending.clear();
            rest = &rest[end + 1..];
        }
        self.take(rest);
    }

    /// Bytes of the line coming in, kept up to `MAX_LINE`
    fn take(&mut self, bytes: &[u8]) {
        if self.skipping {
            return;
        }
        if self.pending.len() + bytes.len() > MAX_LINE {
            self.pending.clear();
            self.skipping = true;
        } else {
            self.pending.extend_from_slice(bytes);
        }
    }

    /// A finished line, without its end of line, at the time it arrived
    pub fn feed_line(&mut self, line: &str, at: Duration) {
        let samples = self.parser.parse(line);
        self.data.push(&samples, at);
    }

    pub fn data(&self) -> &PlotData {
        &self.data
    }

    /// What the chart draws at `at`: the live series, or frozen, the ones it froze on at the
    /// time it froze
    pub fn shown(&self, at: Duration) -> (&PlotData, Duration) {
        match &self.frozen {
            Some((data, frozen_at)) => (data, *frozen_at),
            None => (&self.data, at),
        }
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen.is_some()
    }

    /// Frozen at `at`, or back to the live series
    pub fn freeze(&mut self, at: Option<Duration>) {
        self.frozen = at.map(|at| (self.data.clone(), at));
    }

    /// The series emptied, the line coming in dropped and the freeze over
    pub fn clear(&mut self) {
        self.data.clear();
        self.pending.clear();
        self.skipping = false;
        self.frozen = None;
    }
}

/// Reads `key<separator>value` pairs in a line, or a line of bare numbers when `columns` is on
#[derive(Debug, Clone)]
pub struct PlotParser {
    separator: String,
    keys: Vec<String>,
    columns: bool,
    line_contains: Option<String>,
}

impl PlotParser {
    pub fn new(def: &PlotDef) -> Self {
        Self {
            separator: def.separator.clone(),
            keys: def.keys.clone(),
            columns: def.columns,
            line_contains: def.line_contains.clone(),
        }
    }

    /// The series names and values of a line, in the line's order. A key seen twice: the
    /// first one counts. With `keys`, only those
    pub fn parse<'a>(&'a self, line: &'a str) -> Vec<(&'a str, f64)> {
        if let Some(text) = &self.line_contains
            && !line.contains(text.as_str())
        {
            return Vec::new();
        }
        let mut samples = match self.columns.then(|| bare_columns(line)).flatten() {
            Some(samples) => samples,
            None => self.pairs(line),
        };
        if !self.keys.is_empty() {
            samples.retain(|(name, _)| self.keys.iter().any(|key| key == name));
        }
        samples
    }

    /// Each separator with a word right before it and a number after it, spaces around it
    /// allowed. A pair whose value isn't a number is skipped, the rest of the line is read
    fn pairs<'a>(&self, line: &'a str) -> Vec<(&'a str, f64)> {
        let mut samples: Vec<(&str, f64)> = Vec::new();
        let mut cursor = 0;
        while let Some(found) = line[cursor..].find(self.separator.as_str()) {
            let at = cursor + found;
            let after = at + self.separator.len();
            let key = key_before(&line[cursor..at]);
            let value_start = after + blank_len(&line[after..]);
            match number_prefix(&line[value_start..]) {
                Some((value, len)) if !key.is_empty() => {
                    if !samples.iter().any(|(name, _)| *name == key) {
                        samples.push((key, value));
                    }
                    cursor = value_start + len;
                }
                _ => cursor = after,
            }
        }
        samples
    }
}

/// The word that ends a text, spaces after it allowed: letters, digits, `_` and `.`
fn key_before(text: &str) -> &str {
    let text = text.trim_end_matches([' ', '\t']);
    let start = text
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '.'))
        .map_or(0, |(i, c)| i + c.len_utf8());
    &text[start..]
}

fn blank_len(text: &str) -> usize {
    text.len() - text.trim_start_matches([' ', '\t']).len()
}

/// A line of numbers only, separated by commas, spaces or tabs: series `1`, `2`... up to the
/// series cap. `None` when a piece isn't a whole number (a unit is not allowed here)
fn bare_columns(line: &str) -> Option<Vec<(&'static str, f64)>> {
    let mut samples = Vec::new();
    for piece in line
        .split([',', ' ', '\t'])
        .filter(|piece| !piece.is_empty())
    {
        match number_prefix(piece) {
            Some((value, len)) if len == piece.len() => {
                if let Some(name) = COLUMN_NAMES.get(samples.len()) {
                    samples.push((*name, value));
                }
            }
            _ => return None,
        }
    }
    (!samples.is_empty()).then_some(samples)
}

/// The decimal number a text starts with and its length: sign, decimals, exponent. What
/// follows it (a unit) is left. Hex (`0x1F`) is not a number
fn number_prefix(text: &str) -> Option<(f64, usize)> {
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };

    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let int_digits = digits(end);
    end += int_digits;
    let mut frac_digits = 0;
    if bytes.get(end) == Some(&b'.') {
        frac_digits = digits(end + 1);
        end += 1 + frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    let hex = int_digits == 1 && frac_digits == 0 && bytes[end - 1] == b'0';
    if hex && matches!(bytes.get(end), Some(b'x' | b'X')) {
        return None;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exp_digits = digits(end + 1 + sign);
        if exp_digits > 0 {
            end += 1 + sign + exp_digits;
        }
    }
    text[..end].parse().ok().map(|value| (value, end))
}

/// A series' points, `(x, y)` in the order they came: x is seconds on a time window, the
/// line's sample index on a count window
#[derive(Debug, Clone)]
pub struct PlotSeries {
    pub name: String,
    points: VecDeque<(f64, f64)>,
}

impl PlotSeries {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            points: VecDeque::new(),
        }
    }

    #[cfg(test)]
    pub fn points(&self) -> &VecDeque<(f64, f64)> {
        &self.points
    }

    #[cfg(test)]
    pub fn last(&self) -> Option<f64> {
        self.points.back().map(|(_, y)| *y)
    }

    /// The points from `from` on: the ones a time window's chart still shows between two lines
    pub fn visible(&self, from: f64) -> impl Iterator<Item = (f64, f64)> + '_ {
        let first = self.points.partition_point(|(x, _)| *x < from);
        self.points.range(first..).copied()
    }

    /// The last, lowest and highest values from `from` on, for the legend; none without a point
    pub fn stats(&self, from: f64) -> Option<SeriesStats> {
        let mut values = self.visible(from).map(|(_, y)| y);
        let first = values.next()?;
        let mut stats = SeriesStats {
            last: first,
            min: first,
            max: first,
        };
        for y in values {
            stats.last = y;
            stats.min = stats.min.min(y);
            stats.max = stats.max.max(y);
        }
        Some(stats)
    }

    /// Drops the points before `from`
    fn prune(&mut self, from: f64) {
        while self.points.front().is_some_and(|(x, _)| *x < from) {
            self.points.pop_front();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeriesStats {
    pub last: f64,
    pub min: f64,
    pub max: f64,
}

/// The series of a plot, windowed by time or by count. With `keys`, one series per key in
/// that order from the start; without, a series per new name in the order seen, up to the
/// series cap
#[derive(Debug, Clone)]
pub struct PlotData {
    window: PlotWindow,
    fixed: bool,
    series: Vec<PlotSeries>,
    /// Lines that gave a point: the next line's x on a count window
    samples: u64,
}

impl PlotData {
    pub fn new(def: &PlotDef) -> Self {
        Self {
            window: def.window,
            fixed: !def.keys.is_empty(),
            series: def.keys.iter().map(|key| PlotSeries::new(key)).collect(),
            samples: 0,
        }
    }

    pub fn series(&self) -> &[PlotSeries] {
        &self.series
    }

    /// The x where the window ends, after the last line: `at` on a time window, the next
    /// sample index on a count window
    pub fn x_end(&self, at: Duration) -> f64 {
        match self.window {
            PlotWindow::Time(_) => at.as_secs_f64(),
            PlotWindow::Samples(_) => self.samples as f64,
        }
    }

    /// The x where the window starts, the oldest point it keeps
    pub fn x_start(&self, at: Duration) -> f64 {
        match self.window {
            PlotWindow::Time(window) => at.saturating_sub(window).as_secs_f64(),
            PlotWindow::Samples(count) => self.samples.saturating_sub(count as u64) as f64,
        }
    }

    /// The X axis at `at`: a whole window wide, ending where the window ends, so a window not
    /// full yet fills from the right
    pub fn x_bounds(&self, at: Duration) -> [f64; 2] {
        let width = match self.window {
            PlotWindow::Time(window) => window.as_secs_f64(),
            PlotWindow::Samples(count) => count as f64,
        };
        let end = self.x_end(at);
        [end - width, end]
    }

    /// The Y axis: `min` and `max` when set, else the lowest and highest values from `from`
    /// on, with 5 % of headroom so the curves don't run along the edges (MAX-230), not past 0
    /// when the values don't cross it. A fixed side with the values all past it, or a flat
    /// line, gets a range around it; no value at all, 0 to 1
    pub fn y_bounds(&self, from: f64, min: Option<f64>, max: Option<f64>) -> [f64; 2] {
        let values = (self.series.iter()).flat_map(|series| series.visible(from).map(|(_, y)| y));
        let seen = values.fold(None, |range: Option<(f64, f64)>, y| match range {
            None => Some((y, y)),
            Some((lo, hi)) => Some((lo.min(y), hi.max(y))),
        });
        let below = |lo: f64, pad: f64| match lo >= 0.0 {
            true => (lo - pad).max(0.0),
            false => lo - pad,
        };
        let above = |hi: f64, pad: f64| match hi <= 0.0 {
            true => (hi + pad).min(0.0),
            false => hi + pad,
        };
        match (min, max, seen) {
            (Some(lo), Some(hi), _) => [lo, hi],
            (Some(lo), None, Some((_, hi))) if hi > lo => [lo, above(hi, (hi - lo) * HEADROOM)],
            (Some(lo), None, _) => [lo, lo + 1.0],
            (None, Some(hi), Some((lo, _))) if lo < hi => [below(lo, (hi - lo) * HEADROOM), hi],
            (None, Some(hi), _) => [hi - 1.0, hi],
            (None, None, Some((lo, hi))) if lo < hi => {
                let pad = (hi - lo) * HEADROOM;
                [below(lo, pad), above(hi, pad)]
            }
            (None, None, Some((value, _))) => {
                let pad = if value == 0.0 {
                    1.0
                } else {
                    value.abs() / 10.0
                };
                [value - pad, value + pad]
            }
            (None, None, None) => [0.0, 1.0],
        }
    }

    /// A line's samples, at the time it arrived; the points out of the window go. A line
    /// without a sample doesn't move a count window
    pub fn push(&mut self, samples: &[(&str, f64)], at: Duration) {
        let x = match self.window {
            PlotWindow::Time(_) => at.as_secs_f64(),
            PlotWindow::Samples(_) => self.samples as f64,
        };
        let mut pushed = false;
        for &(name, value) in samples {
            let Some(series) = self.series_for(name) else {
                continue;
            };
            series.points.push_back((x, value));
            if series.points.len() > MAX_POINTS {
                series.points.pop_front();
            }
            pushed = true;
        }
        if pushed && matches!(self.window, PlotWindow::Samples(_)) {
            self.samples += 1;
        }
        let from = self.x_start(at);
        for series in &mut self.series {
            series.prune(from);
        }
    }

    pub fn clear(&mut self) {
        for series in &mut self.series {
            series.points.clear();
        }
        if !self.fixed {
            self.series.clear();
        }
        self.samples = 0;
    }

    /// The series of a name, a new one while there's room when the keys aren't fixed
    fn series_for(&mut self, name: &str) -> Option<&mut PlotSeries> {
        match self.series.iter().position(|series| series.name == name) {
            Some(i) => Some(&mut self.series[i]),
            None if !self.fixed && self.series.len() < MAX_SERIES => {
                self.series.push(PlotSeries::new(name));
                self.series.last_mut()
            }
            None => None,
        }
    }
}

impl App {
    /// The plots of `plots.toml`, at start and on a reload (MAX-222). The plot shown is
    /// rebuilt when its definition changed, its points dropped, and turned off when it is
    /// gone from the file
    pub fn set_plots(&mut self, plots: PlotsConfig) {
        if let Some(plot) = &self.plot {
            match plots.get(&plot.name) {
                None => {
                    let name = plot.name.clone();
                    self.plot = None;
                    let msg = format!("Plot {name} is gone from plots.toml: plot off");
                    self.notify_plot(ToastLevel::Warn, msg);
                }
                Some(def) if def != plot.def() => {
                    self.plot = Some(Plot::new(plot.name.clone(), def));
                    self.plot_started = Instant::now();
                }
                Some(_) => {}
            }
        }
        self.plots = plots;
    }

    pub fn plots(&self) -> &PlotsConfig {
        &self.plots
    }

    /// The plot shown, fed from RX
    pub fn plot(&self) -> Option<&Plot> {
        self.plot.as_ref()
    }

    /// The plot `config.toml` or `--plot` names, at start: one that isn't in `plots.toml` is
    /// said, the plot stays off
    pub fn show_start_plot(&mut self) {
        let Some(name) = self.settings.plot.clone() else {
            return;
        };
        if self.plots.get(&name).is_some() {
            self.show_plot(Some(&name));
        } else {
            let msg = format!("No plot named '{name}' in plots.toml: plot off");
            self.notify_plot(ToastLevel::Warn, msg);
        }
    }

    /// Shows the plot named, on fresh series, or none (`plot off`). The plot shown already
    /// keeps its points
    pub fn show_plot(&mut self, name: Option<&str>) {
        let Some(name) = name else {
            if self.plot.take().is_some() {
                self.notify_plot(ToastLevel::Info, "Plot off");
            }
            return;
        };
        if self.plot.as_ref().is_some_and(|plot| plot.name == name) {
            return;
        }
        let Some(def) = self.plots.get(name) else {
            let msg = format!("No plot named '{name}' in plots.toml");
            self.notify_plot(ToastLevel::Warn, msg);
            return;
        };
        self.plot = Some(Plot::new(name.to_string(), def));
        self.plot_started = Instant::now();
        self.notify_plot(ToastLevel::Info, format!("Plot {name}"));
    }

    /// The plot's toast: showing, switching, freezing and closing it follow each other in one
    /// box (MAX-234)
    pub fn notify_plot(&mut self, level: ToastLevel, msg: impl Into<String>) {
        self.notify_keyed("plot", level, msg);
    }

    /// The plot after the one shown in the file's order, off after the last: what `plot`
    /// alone shows
    pub fn next_plot(&self) -> Option<&str> {
        let mut names = self.plots.plots.iter().map(|(name, _)| name.as_str());
        match &self.plot {
            None => names.next(),
            Some(plot) => names.skip_while(|name| *name != plot.name).nth(1),
        }
    }

    /// `plot-next` / `plot-prev` and the panel's arrows: the next or previous plot, from the
    /// last back to the first and the other way, never off (MAX-230): the panel's `✕`,
    /// `plot off` and `plot` turn it off
    pub fn step_plot(&mut self, forward: bool) {
        let names = &self.plots.plots;
        if names.is_empty() {
            self.notify_plot(ToastLevel::Warn, "plots.toml has no plot");
            return;
        }
        let shown = (self.plot.as_ref())
            .and_then(|plot| names.iter().position(|(name, _)| *name == plot.name));
        let count = names.len();
        let index = match (shown, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(at), true) => (at + 1) % count,
            (Some(at), false) => (at + count - 1) % count,
        };
        let name = names[index].0.clone();
        self.show_plot(Some(&name));
    }

    /// The time on the plot's clock: since it was shown or last cleared
    pub fn plot_clock(&self) -> Duration {
        self.plot_started.elapsed()
    }

    /// `plot-freeze`: the chart stops moving or jumps back to now; the data is collected
    /// either way
    pub fn toggle_plot_freeze(&mut self) {
        let at = self.plot_clock();
        let Some(plot) = &mut self.plot else {
            self.notify_plot(ToastLevel::Warn, "No plot shown");
            return;
        };
        let frozen = !plot.is_frozen();
        plot.freeze(frozen.then_some(at));
        let name = plot.name.clone();
        let state = if frozen { "frozen" } else { "live" };
        self.notify_plot(ToastLevel::Info, format!("Plot {name} {state}"));
    }

    /// RX bytes for the plot shown, at the time they came in on its clock
    pub fn feed_plot(&mut self, chunk: &[u8]) {
        if let Some(plot) = &mut self.plot {
            plot.feed(chunk, self.settings.encoding, self.plot_started.elapsed());
        }
    }

    /// The plot starts over with the views: a device switched to, a replay started or
    /// restarted or stopped
    pub fn clear_plot(&mut self) {
        if let Some(plot) = &mut self.plot {
            plot.clear();
            self.plot_started = Instant::now();
        }
    }
}

#[cfg(test)]
#[path = "../tests/app/plots.rs"]
mod tests;
