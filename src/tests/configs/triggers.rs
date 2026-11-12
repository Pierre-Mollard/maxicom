use super::*;

/// The default's two examples that send something (`login`, `u-boot`): the tests of arming,
/// the browser and the status bar were written on them, before the `panic` one (MAX-228)
pub fn two_examples() -> TriggersConfig {
    let mut config = TriggersConfig::default();
    config.rules.remove("panic");
    config
}

fn compile(toml: &str) -> Result<Vec<Trigger>, String> {
    let config: TriggersConfig = toml::from_str(toml).map_err(|e| e.to_string())?;
    config.compile().map_err(|e| e.to_string())
}

/// The default file round-trips and compiles: the three examples, disarmed, the login one
/// rearming, the panic one rearming and silent (MAX-228), its `notify` its only notice.
#[test]
fn default_examples() {
    let text = toml::to_string_pretty(&TriggersConfig::default()).unwrap();
    let config: TriggersConfig = toml::from_str(&text).unwrap();
    assert_eq!(config, TriggersConfig::default());
    let triggers = config.compile().unwrap();
    let names: Vec<_> = triggers.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["login", "panic", "u-boot"]);
    assert!(triggers.iter().all(|t| !t.is_armed()));
    assert!(triggers[0].rearm && triggers[1].rearm && !triggers[2].rearm);
    assert_eq!(triggers[0].invocations.len(), 1);
    assert!(triggers[1].silent && !triggers[0].silent && !triggers[2].silent);
    assert_eq!(triggers[1].run, "notify error \"the board panicked\"");
}

/// `rearm` and `armed` are optional; `match` takes the escapes, `run` a command line.
#[test]
fn rule_fields() {
    let triggers = compile(
        r#"
            [menu]
            match = '\x1b[7mBoot menu'
            run = "send-hex 1b; sleep 50; send 1"
            armed = true
            "#,
    )
    .unwrap();
    let menu = &triggers[0];
    assert!(menu.is_armed() && !menu.rearm);
    assert_eq!(menu.invocations.len(), 3);
    let mut menu = menu.clone();
    assert!(menu.fires(b"\x1b[7mBoot menu\x1b[0m"));
}

/// A bad rule fails the load, naming it; so does a field that isn't one.
#[test]
fn bad_rules() {
    let rule = |text: &str, run: &str| format!("[r]\nmatch = '{text}'\nrun = '{run}'\n");
    assert_eq!(
        compile(&rule("", "send x")).unwrap_err(),
        "[r]: match is empty"
    );
    assert_eq!(
        compile(&rule("\\q", "send x")).unwrap_err(),
        "[r]: match: bad escape '\\q'"
    );
    assert_eq!(compile(&rule("x", " ; ")).unwrap_err(), "[r]: run is empty");
    assert_eq!(
        compile(&rule("x", "rst on")).unwrap_err(),
        "[r]: run: unknown command 'rst'"
    );
    let typo = compile("[r]\nmatch = 'x'\nrun = 'send x'\nrearn = true\n").unwrap_err();
    assert!(typo.contains("unknown field `rearn`"), "{typo}");
}
