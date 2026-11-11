use super::*;

fn load(toml: &str) -> Result<PlotsConfig, String> {
    let config: PlotsConfig = toml::from_str(toml).map_err(|e| e.to_string())?;
    config.check().map_err(|e| e.to_string())?;
    Ok(config)
}

/// MAX-222: the default file round-trips and checks: `key-value` on 30 s, `arduino` with
/// `:`, bare columns and 500 samples, in this order.
#[test]
fn default_plots() {
    let text = toml::to_string_pretty(&PlotsConfig::default()).unwrap();
    let config = load(&text).unwrap();
    assert_eq!(config, PlotsConfig::default());
    let names: Vec<_> = config.plots.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["key-value", "arduino"]);
    let arduino = &config.plots[1].1;
    assert_eq!(arduino.separator, ":");
    assert!(arduino.columns);
    assert_eq!(arduino.window, PlotWindow::Samples(500));
    assert_eq!(
        config.plots[0].1.window,
        PlotWindow::Time(Duration::from_secs(30))
    );
}

/// MAX-222: the default file as written: short (fields at their default left out, but the
/// separator and the window), with its comments, and it loads back the same.
#[test]
fn default_file_text() {
    let text = toml::to_string_pretty(&PlotsConfig::default()).unwrap();
    let text = with_comments(&text, PLOTS_COMMENTS);
    assert!(
        text.starts_with("# Numbers charted from the received lines: "),
        "{text}"
    );
    assert!(
        text.contains("[key-value]\nseparator = \"=\"\nwindow = \"30s\" # a duration"),
        "{text}"
    );
    assert!(
        text.contains("[arduino]\nseparator = \":\"\ncolumns = true # a line of bare numbers"),
        "{text}"
    );
    assert!(text.contains("window = 500 # a number"), "{text}");
    assert_eq!(load(&text).unwrap(), PlotsConfig::default());
}

/// MAX-222: every field, and the plots kept in the file's order (not sorted).
#[test]
fn plot_fields() {
    let config = load(
        r##"
            [pid]
            separator = "->"
            keys = ["err", "out"]
            line_contains = "PID"
            window = "2m 30s"
            min = -1.5
            max = 100
            colors = { err = "Red", out = "#00FF00" }

            [adc]
            columns = true
            window = 50
            "##,
    )
    .unwrap();
    let names: Vec<_> = config.plots.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["pid", "adc"]);
    let pid = &config.plots[0].1;
    assert_eq!(pid.separator, "->");
    assert_eq!(pid.keys, ["err", "out"]);
    assert_eq!(pid.line_contains.as_deref(), Some("PID"));
    assert_eq!(pid.window, PlotWindow::Time(Duration::from_secs(150)));
    assert_eq!((pid.min, pid.max), (Some(-1.5), Some(100.0)));
    assert_eq!(pid.colors["err"], Color::Red);
    assert_eq!(pid.colors["out"], Color::Rgb(0, 255, 0));

    // left out: the defaults
    let adc = &config.plots[1].1;
    assert_eq!(adc.separator, "=");
    assert!(adc.keys.is_empty() && adc.columns && adc.colors.is_empty());
    assert_eq!(adc.window, PlotWindow::Samples(50));
}

/// MAX-222: a plot that can't draw anything fails the load, naming it; so does a plot named
/// `off`, the word of `plot off`.
#[test]
fn bad_plots() {
    let plot = |fields: &str| load(&format!("[p]\n{fields}\n")).unwrap_err();
    assert_eq!(plot("separator = \"\""), "[p]: separator is empty");
    assert_eq!(plot("line_contains = \"\""), "[p]: line_contains is empty");
    assert_eq!(plot("window = 0"), "[p]: window is 0");
    assert_eq!(plot("window = \"0s\""), "[p]: window is 0");
    assert_eq!(plot("min = 5\nmax = 5"), "[p]: min 5 is not below max 5");
    assert_eq!(plot("keys = [\"a\", \" \"]"), "[p]: key ' ' is blank");
    let nine = (1..=9).map(|i| format!("\"k{i}\"")).collect::<Vec<_>>();
    assert_eq!(
        plot(&format!("keys = [{}]", nine.join(", "))),
        "[p]: 9 keys, at most 8"
    );
    assert_eq!(
        load("[off]\n").unwrap_err(),
        "[off]: 'off' turns the plot off, name it otherwise"
    );
}

/// MAX-222: a window that is neither a duration nor a count is a parse error that says so.
#[test]
fn bad_window() {
    let err = load("[p]\nwindow = \"soon\"\n").unwrap_err();
    assert!(err.contains("window 'soon'"), "{err}");
    let err = load("[p]\nwindow = -3\n").unwrap_err();
    assert!(err.contains("not a number of samples"), "{err}");
    let err = load("[p]\nwindow = 1.5\n").unwrap_err();
    assert!(
        err.contains("a duration (\"30s\") or a number of samples"),
        "{err}"
    );
}

/// MAX-222: a misspelled field doesn't fail the load (it takes its default) but the
/// unknown key warning names it, with its plot.
#[test]
fn unknown_field_is_named() {
    let text = "[p]\nseperator = \":\"\n";
    assert_eq!(load(text).unwrap().plots[0].1.separator, "=");
    assert_eq!(
        crate::configs::unknown_keys::<PlotsConfig>(text),
        ["p.seperator"]
    );
}
