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
            "\nkeybindings_path, highlights_path, triggers_path or links_path:",
            "\ntheme_path = \"~/maxicom/dark.toml\"",
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
    ("prefix_actions", "After the prefix, in both modes"),
    (
        "direct_actions",
        "Without the prefix, in both modes: these keys never reach the device",
    ),
    ("line_mode", "In line mode, while typing in the input box"),
    (
        "terminal_mode",
        "In terminal mode, before the key goes to the device",
    ),
    ("search_mode", "While the search bar is open"),
    ("copy_mode", "In copy mode"),
    ("dialog_actions", "In the popups"),
];

pub const HIGHLIGHTS_COMMENTS: &[(&str, &str)] = &[(
    "",
    concat!(
        "Words colored in the received text: ",
        guide!("highlights.html"),
        "\nA word = its color, or a table = a style, written as in theme.toml.",
        "\nQuote a word with spaces or symbols. Ctrl-a r reloads this file.",
    ),
)];

pub const TRIGGERS_COMMENTS: &[(&str, &str)] = &[(
    "",
    concat!(
        "Run a command line when a text comes in: ",
        guide!("triggers.html"),
        "\nmatch: the text to wait for; run: the command line to run",
        "\nrearm: stay armed after firing; armed: armed at startup",
    ),
)];

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

/// `text`, a TOML file as `toml::to_string_pretty` writes it, with the comments of `comments`,
/// and each array on one line so that it can take its comment. A multi-line string can't: its
/// comment goes above it, and its lines are left alone
pub fn with_comments(text: &str, comments: &[(&str, &str)]) -> String {
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
