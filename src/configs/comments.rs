//! The comments of the default config files (MAX-209). Serde generates the keys and values,
//! these lists add what can't be guessed: the guide's chapter of a section, the values a key
//! takes. A comment never mentions a default value, so it can't go stale.
//! A list holds `("", text)` for the top of the file and `("section", text)` for a section,
//! above them, and `("section.key", text)` for a key, one line at the end of its own.

macro_rules! guide {
    ($page:literal) => {
        concat!("https://pierre-mollard.github.io/maxicom/", $page)
    };
}

pub const CONFIG_COMMENTS: &[(&str, &str)] = &[
    (
        "",
        concat!(
            "maxicom's settings: ",
            guide!("settings.html"),
            "\nA setting left out takes its default. Ctrl-a r reloads this file, except for ",
            "a few settings:\n",
            guide!("settings.html#settings-that-need-a-restart"),
            "\nDurations are written \"500ms\", \"30s\", \"4h\" or \"1day\".",
            "\n\nPut a file elsewhere by adding, at the top of this file, theme_path,",
            "\nkeybindings_path, highlights_path, triggers_path, links_path or plots_path:",
            "\ntheme_path = \"~/maxicom/dark.toml\"",
            "\n\nStart with a plot of plots.toml shown: plot = \"<name>\"",
        ),
    ),
    ("encoding", "values: utf8, ascii, cp437, latin1"),
    ("mode", "values: line, terminal"),
    ("dual_mode_buffering", "values: always_both, active_only"),
    ("rx_buffer_size", "512 to 65536"),
    ("inactivity_detach_timeout", "\"0s\": never"),
    ("line_mode", guide!("line-mode.html")),
    ("line_mode.tx_eol", "values: crlf, cr, lf"),
    (
        "line_mode.display_mode",
        "values: pretty, clean, raw, hex, octal, binary",
    ),
    ("line_mode.wrap_mode", "values: word, char, truncate"),
    ("line_mode.wrap_indent", "values: none, prefix, timestamp"),
    ("line_mode.prefix_place", "values: inset, border, none"),
    ("terminal_mode", guide!("terminal-mode.html")),
    (
        "terminal_mode.dimension_mode",
        "values: grow_only, fixed_cols, match_view, fixed",
    ),
    (
        "terminal_mode.answer_groups",
        "values: status, identity, size, modes",
    ),
    ("capture", guide!("capture.html#settings")),
    ("sessions", guide!("capture.html#settings")),
    ("sessions.keep", "0: no limit"),
    ("sessions.max_size", "K, M, G or T, \"0\": no limit"),
    ("sessions.restore", "values: never, ask, always"),
    ("sessions.restore_max_age", "\"0s\": no limit"),
    ("sessions.group_by", "values: month, week, day"),
    ("screenshot", guide!("capture.html#settings")),
    ("transfers", guide!("file-transfers.html#downloading")),
    (
        "transfers.dir",
        "where download puts its files, empty: the current directory",
    ),
    (
        "transfers.xmodem_name",
        "strftime: an XMODEM file comes with no name",
    ),
    ("link", guide!("links.html#default-link")),
    ("link.bits", "5 to 8"),
    ("link.parity", "values: none, even, odd"),
    ("link.stop_bits", "1 or 2"),
    ("link.flow", "values: none, rtscts, xonxoff"),
    ("ui", guide!("settings.html#ui")),
    ("ui.hide_borders_below", "[cols, rows], 0 turns a side off"),
    (
        "ui.hide_scrollbar_below",
        "[cols, rows], 0 turns a side off",
    ),
    (
        "ui.hide_status_bar_below",
        "[cols, rows], 0 turns a side off",
    ),
    ("ui.status_bar", guide!("status-bar.html")),
    ("ui.status_bar.layout", "values: line, band, border, box"),
    ("ui.marks", guide!("marks.html")),
    ("raw_side", guide!("raw-side-panel.html")),
    ("raw_side.view", "values: off, hex, octal, binary"),
    ("raw_side.orientation", "values: right, bottom"),
    ("raw_side.split_percent", "10 to 85"),
    ("plot_side", guide!("plots.html#size-and-position")),
    ("plot_side.orientation", "values: right, bottom"),
    ("plot_side.split_percent", "10 to 85"),
    ("command_line", guide!("command-line.html")),
    ("command_line.layout", "values: popup, status_bar"),
];

pub const THEME_COMMENTS: &[(&str, &str)] = &[(
    "",
    concat!(
        "maxicom's colors: ",
        guide!("theme.html"),
        "\nAn entry left out takes its default. Ctrl-a r reloads this file.",
        "\n\nA color: Red, LightBlue, Reset, \"#RRGGBB\", or \"75\" from the 256-color palette",
        "\nA style, except for the entries that take a color only:",
        "\n{ fg = \"Black\", bg = \"#FFB86C\", bold = true }",
        "\nStyle fields: fg, bg, underline_color, bold, dim, italic, underlined, reversed, crossed",
    ),
)];

pub const KEYMAP_COMMENTS: &[(&str, &str)] = &[
    (
        "",
        concat!(
            "maxicom's keys: ",
            guide!("keybindings.html"),
            "\nThe help (Ctrl-a z) lists the keys in use. Ctrl-a r reloads this file.",
            "\nA section here replaces its default whole: keep the keys you want in it.",
            "\nThe actions: ",
            guide!("keybindings.html#actions"),
        ),
    ),
    (
        "prefixed",
        concat!(
            "After the prefix, in both modes. A key takes an action, a command that runs\n",
            "without arguments (break, plot...) or a named command of [commands]",
        ),
    ),
    (
        "direct",
        concat!(
            "Without the prefix, in both modes: these keys never reach the device.\n",
            "A key takes an action, a command that runs without arguments or a named\n",
            "command, which runs in copy mode too",
        ),
    ),
    (
        "line_mode",
        concat!(
            "In line mode, while typing in the input box. A key takes a line mode action\n",
            "or a named command, not the actions and commands of [prefixed] and [direct]",
        ),
    ),
    (
        "terminal_mode",
        concat!(
            "In terminal mode, before the key goes to the device. A key takes a terminal\n",
            "mode action or a named command, not the actions and commands of [prefixed]",
        ),
    ),
    (
        "search_mode",
        concat!(
            "While the search bar is open. A key takes a search bar action only: the\n",
            "other keys type the search",
        ),
    ),
    (
        "copy_mode",
        concat!(
            "In copy mode. A key takes a copy mode action or a named command, not the\n",
            "actions and commands of [prefixed] and [direct]",
        ),
    ),
    (
        "dialog",
        "In the popups. A key takes a popup action only: in a text field, the other\nkeys type",
    ),
    (
        "commands",
        concat!(
            "Command lines under a name, run by a key bound to the name or typed on the\n",
            "command line: ",
            guide!("keybindings.html#named-commands"),
            "\nname = { run = \"commands; chained; with ;\", help = \"what the help shows\" }",
            "\nhelp is optional. A name is one word (letters, digits, - and _) that no",
            "\ncommand nor action has. A line runs commands, not other named ones.",
            "\nThe default commands stay: one here with the same name takes its place",
        ),
    ),
];

pub const HIGHLIGHTS_COMMENTS: &[(&str, &str)] = &[
    (
        "",
        concat!(
            "Words colored in the received text: ",
            guide!("highlights.html"),
            "\nA word = its color, or a table = a style, written as in theme.toml.",
            "\nA table with words = [...] gives them all its style; ignore_case and whole_word",
            "\nset how they match. Later entries are drawn over earlier ones.",
            "\nQuote a word with spaces or symbols. Ctrl-a r reloads this file.",
            "\n\nline_start = [...] in place of words colors the whole lines that start with",
            "\none of them, the words drawn over. For logs that put the level first:",
            "\n[error-lines]",
            "\nline_start = [\"ERROR:\", \"[E]\"]",
            "\nfg = \"Red\"",
        ),
    ),
    ("INFO", "a word on its own: its color"),
    (
        "errors",
        "A group: each word of words in this style, any case, whole words only",
    ),
    (
        "TIMEOUT",
        "A word on its own in a style: a table without words styles its name",
    ),
];

pub const TRIGGERS_COMMENTS: &[(&str, &str)] = &[(
    "",
    concat!(
        "Run a command line when a text comes in: ",
        guide!("triggers.html"),
        "\nmatch: the text to wait for; run: the command line to run",
        "\nrearm: stay armed after firing; armed: armed at startup",
        "\nsilent: no notice when it fires, its run can notify",
    ),
)];

pub const PLOTS_COMMENTS: &[(&str, &str)] = &[
    (
        "",
        concat!(
            "Numbers charted from the received lines: ",
            guide!("plots.html"),
            "\nA table per plot. separator: what joins a key to its value (\"=\", \":\", \"->\")",
            "\nkeys = [\"temp\", \"hum\"]: only these series, in this order; left out: every key",
            "\nfound, up to 8. columns = true: a line of bare numbers gives series 1, 2, 3...",
            "\nline_contains = \"PID\": only the lines with this text",
            "\nmin / max: a fixed Y range, autoscale without them",
            "\ncolors = { temp = \"Red\" }: a series' color, else the theme's link colors",
            "\nCtrl-a r reloads this file.",
        ),
    ),
    (
        "key-value",
        "every key=value pair of every line: temp=23.4 hum=51",
    ),
    ("key-value.window", "a duration: the X axis is time"),
    (
        "serial-plotter",
        "the serial plotter format: temp:23.4,hum:51 or 23.4 51",
    ),
    (
        "serial-plotter.columns",
        "a line of bare numbers: series 1, 2, 3...",
    ),
    (
        "serial-plotter.window",
        "a number: the last samples of each series",
    ),
    (
        "sensors",
        "two series by name, on a fixed range, in their colors",
    ),
    ("sensors.keys", "only these series, in this order"),
    ("sensors.max", "with min: a fixed Y range"),
    (
        "sensors.colors",
        "a color per series, else the theme's link colors",
    ),
    (
        "tagged",
        "only the lines tagged for it: [plot] err=-1.5 out=40",
    ),
    ("tagged.line_contains", "only the lines with this text"),
];

fn write_comment(out: &mut String, comment: &str) {
    for line in comment.lines() {
        out.push_str(if line.is_empty() { "#" } else { "# " });
        out.push_str(line);
        out.push('\n');
    }
}

/// What ends a string that goes on over the next lines, when the value starts one
fn multi_line_string_end(value: &str) -> Option<&'static str> {
    let quotes = ["'''", "\"\"\""];
    let quote = quotes.into_iter().find(|quote| value.starts_with(quote))?;
    let closed = value.len() > quote.len() && value.ends_with(quote);
    (!closed).then_some(quote)
}

/// The sections whose tables are written inline, one a line: `[commands]` (MAX-239)
const INLINE_SECTIONS: &[&str] = &["commands"];

/// `text` with the tables of the `INLINE_SECTIONS` written inline under their section's
/// header: `[commands.ping]` and its keys become `ping = { run = "send AT" }` under
/// `[commands]`
fn inline_tables(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.lines().peekable();
    let mut open = None;
    while let Some(line) = lines.next() {
        let table = (line
            .strip_prefix('[')
            .and_then(|name| name.strip_suffix(']')))
        .and_then(|name| name.split_once('.'))
        .filter(|(section, _)| INLINE_SECTIONS.contains(section));
        let Some((section, name)) = table else {
            if open.take().is_some() && !line.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
            out.push('\n');
            continue;
        };
        if open != Some(section) {
            out.push_str(&format!("[{section}]\n"));
            open = Some(section);
        }
        let mut pairs = Vec::new();
        while let Some(line) = lines.next_if(|line| !line.is_empty() && !line.starts_with('[')) {
            let mut pair = line.to_string();
            if let Some(end) = line
                .split_once(" = ")
                .and_then(|(_, v)| multi_line_string_end(v))
            {
                for line in lines.by_ref() {
                    pair.push('\n');
                    pair.push_str(line);
                    if line.ends_with(end) {
                        break;
                    }
                }
            }
            pairs.push(pair);
        }
        out.push_str(&format!("{name} = {{ {} }}\n", pairs.join(", ")));
        // the blank line between two tables of the section
        while lines.next_if(|line| line.is_empty()).is_some() {}
    }
    out
}

/// `text`, a TOML file as `toml::to_string_pretty` writes it, with the comments of `comments`,
/// and each array on one line so that it can take its comment. A multi-line string can't: its
/// comment goes above it, and its lines are left alone. The tables of `[commands]` are
/// written inline, one a line (MAX-239)
pub fn with_comments(text: &str, comments: &[(&str, &str)]) -> String {
    let text = &inline_tables(text);
    let comment = |path: &str| comments.iter().find(|(key, _)| *key == path).map(|c| c.1);
    let mut out = String::with_capacity(text.len());
    if let Some(header) = comment("") {
        write_comment(&mut out, header);
        out.push('\n');
    }

    let mut section = "";
    let mut written = Vec::new();
    let mut first_line = true;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some(name) = line.strip_prefix('[') {
            let name = name.strip_suffix(']').unwrap_or(name);
            // Two blank lines between top-level sections
            if !first_line && name.split('.').next() != section.split('.').next() {
                out.push('\n');
            }
            section = name;
            // A parent without its own header, as `[search]`, comes with its first child
            let parents = section.match_indices('.').map(|(end, _)| &section[..end]);
            for name in parents.chain([section]) {
                if !written.contains(&name) {
                    written.push(name);
                    if let Some(text) = comment(name) {
                        write_comment(&mut out, text);
                    }
                }
            }
            out.push_str(line);
        } else if let Some((key, value)) = line.split_once(" = ") {
            let path = match section {
                "" => key.to_owned(),
                _ => format!("{section}.{key}"),
            };
            let key_comment = comment(&path);
            if let Some(end) = multi_line_string_end(value) {
                if let Some(text) = key_comment {
                    write_comment(&mut out, text);
                }
                out.push_str(line);
                for line in lines.by_ref() {
                    out.push('\n');
                    out.push_str(line);
                    if line.ends_with(end) {
                        break;
                    }
                }
            } else {
                if value == "[" {
                    let items = lines.by_ref().take_while(|line| *line != "]");
                    let items = items.map(|line| line.trim().trim_end_matches(','));
                    let items = items.collect::<Vec<_>>().join(", ");
                    out.push_str(&format!("{key} = [{items}]"));
                } else {
                    out.push_str(line);
                }
                if let Some(text) = key_comment {
                    out.push_str(" # ");
                    out.push_str(text);
                }
            }
        } else {
            out.push_str(line);
        }
        first_line = false;
        out.push('\n');
    }
    out
}

#[cfg(test)]
#[path = "../tests/configs/comments.rs"]
mod tests;
