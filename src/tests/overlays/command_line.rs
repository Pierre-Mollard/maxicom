use super::*;

/// A command line with `text` typed: its completion listed
fn line(text: &str) -> CommandLine {
    let mut command_line = CommandLine {
        input: Input::new(text.to_string()),
        listed: !text.is_empty(),
        ..CommandLine::default()
    };
    command_line.refresh();
    command_line
}

#[test]
fn confirm_runs_the_parsed_commands() {
    match line("help; toggle-mouse").handle_action(DialogAction::Confirm) {
        OverlayResponse::QueueCommands { line, invocations } => {
            assert_eq!(line, "help; toggle-mouse");
            let names: Vec<_> = invocations.iter().map(|inv| inv.spec.name).collect();
            assert_eq!(names, ["help", "toggle-mouse"]);
        }
        _ => panic!("expected QueueCommands"),
    }
}

#[test]
fn empty_line_closes() {
    for text in ["", "  ", " ; "] {
        let response = line(text).handle_action(DialogAction::Confirm);
        assert!(matches!(response, OverlayResponse::Close), "{text:?}");
    }
}

/// A line that doesn't parse warns with the error and keeps the prompt
#[test]
fn bad_line_warns_and_stays() {
    match line("help; nope").handle_action(DialogAction::Confirm) {
        OverlayResponse::HandledNotify { message, warn } => {
            assert_eq!(message, "unknown command 'nope'");
            assert!(warn);
        }
        _ => panic!("expected HandledNotify"),
    }
}

#[test]
fn typing_and_pasting() {
    let mut command_line = CommandLine::new(Vec::new(), Vec::new(), Vec::new());
    let key = |c| crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char(c));
    command_line.handle_action(DialogAction::HandleKey(key('h')));
    command_line.handle_action(DialogAction::Paste("elp".to_string()));
    assert_eq!(command_line.input.value(), "help");
    let response = command_line.handle_action(DialogAction::Close);
    assert!(matches!(response, OverlayResponse::Close));
}

fn value(command_line: &CommandLine) -> &str {
    command_line.input.value()
}

fn key(code: KeyCode) -> DialogAction {
    DialogAction::HandleKey(crossterm::event::KeyEvent::from(code))
}

/// MAX-163: the command line opens listing nothing; Tab lists every command, none
/// selected, then the next Tab puts the first in; typing lists, and erasing it all hides
/// the list again.
/// How: Tab twice on a fresh line; then a letter typed and erased.
#[test]
fn empty_line_lists_on_tab() {
    let mut command_line = CommandLine::new(Vec::new(), Vec::new(), Vec::new());
    assert!(command_line.completion.candidates.is_empty());
    command_line.handle_action(DialogAction::CycleNext);
    assert_eq!(
        command_line.completion.candidates.len(),
        commands::COMMANDS.len()
    );
    assert_eq!((value(&command_line), command_line.selected), ("", None));
    command_line.handle_action(DialogAction::CycleNext);
    assert_eq!(command_line.selected, Some(0));

    let mut command_line = CommandLine::new(Vec::new(), Vec::new(), Vec::new());
    command_line.handle_action(key(KeyCode::Char('t')));
    assert!(!command_line.completion.candidates.is_empty());
    command_line.handle_action(key(KeyCode::Backspace));
    assert!(command_line.completion.candidates.is_empty());
}

/// Tab, Down, Shift-Tab and Up put the candidates of the typed word in its place, in turn;
/// typing lists the new word's
#[test]
fn cycle_candidates() {
    let mut command_line = line("help; toggle-d");
    command_line.handle_action(DialogAction::CycleNext);
    assert_eq!(value(&command_line), "help; toggle-display-mode");
    command_line.handle_action(key(KeyCode::Down));
    assert_eq!(value(&command_line), "help; toggle-display-raw");
    assert_eq!(command_line.selected, Some(1));
    command_line.handle_action(key(KeyCode::Up));
    command_line.handle_action(DialogAction::CyclePrev);
    assert_eq!(value(&command_line), "help; toggle-display-timestamp");
    assert_eq!(command_line.completion.candidates.len(), 3);

    command_line.handle_action(key(KeyCode::Backspace));
    assert_eq!(command_line.selected, None);
    assert_eq!(command_line.completion.candidates.len(), 1);
}

/// The word before the cursor is replaced, the text after it stays
#[test]
fn cycle_in_the_middle() {
    let mut command_line = CommandLine {
        input: Input::new("toggle-d; help".to_string()).with_cursor(8),
        listed: true,
        ..CommandLine::default()
    };
    command_line.refresh();
    command_line.handle_action(DialogAction::CycleNext);
    assert_eq!(value(&command_line), "toggle-display-mode; help");
    assert_eq!(command_line.input.cursor(), 19);
}

/// A lone candidate, or a click on one, is taken with a space after it, and the next word
/// is completed
#[test]
fn take_a_candidate() {
    let mut command_line = line("toggle-mou");
    command_line.handle_action(DialogAction::CycleNext);
    assert_eq!(value(&command_line), "toggle-mouse ");
    assert!(command_line.completion.candidates.is_empty());

    let mut command_line = line("toggle-d");
    command_line.handle_action(DialogAction::Pick(PopupTarget::Completion(1)));
    assert_eq!(value(&command_line), "toggle-display-raw ");
    command_line.handle_action(DialogAction::Pick(PopupTarget::Completion(5)));
    assert_eq!(value(&command_line), "toggle-display-raw ");
}

/// Up and Down move the cursor as usual with nothing to complete
#[test]
fn arrows_without_candidates() {
    let mut command_line = line("zz");
    command_line.handle_action(key(KeyCode::Up));
    command_line.handle_action(key(KeyCode::Down));
    assert_eq!(value(&command_line), "zz");
}

/// Up and Down walk the history as the TX input: from the newest, the typed line back past
/// it; an edit starts from the edited line. MAX-163: only the lines starting with the text
/// typed when the walk started.
/// How: a history of four lines, walked from an empty line, then from "send " (no
/// completion), then from a recalled line edited.
#[test]
fn walk_the_history() {
    let history = ["help", "send a", "toggle-mouse", "send b"];
    let new = || {
        CommandLine::new(
            history.iter().map(|line| line.to_string()).collect(),
            Vec::new(),
            Vec::new(),
        )
    };
    let up = |command_line: &mut CommandLine| {
        command_line.handle_action(key(KeyCode::Up));
        command_line.input.value().to_string()
    };
    let down = |command_line: &mut CommandLine| {
        command_line.handle_action(key(KeyCode::Down));
        command_line.input.value().to_string()
    };

    let mut command_line = new();
    assert_eq!(down(&mut command_line), "");
    assert_eq!(up(&mut command_line), "send b");
    assert_eq!(up(&mut command_line), "toggle-mouse");
    assert_eq!(up(&mut command_line), "send a");
    assert_eq!(up(&mut command_line), "help");
    assert_eq!(up(&mut command_line), "help");
    assert_eq!(down(&mut command_line), "send a");

    let mut command_line = new();
    command_line.handle_action(DialogAction::Paste("send ".into()));
    assert!(command_line.completion.candidates.is_empty());
    assert_eq!(up(&mut command_line), "send b");
    assert_eq!(up(&mut command_line), "send a");
    assert_eq!(up(&mut command_line), "send a");
    assert_eq!(down(&mut command_line), "send b");
    assert_eq!(down(&mut command_line), "send ");
    assert_eq!(down(&mut command_line), "send ");

    command_line.handle_action(DialogAction::Paste("x".into()));
    assert_eq!(up(&mut command_line), "send x", "no line starts with it");

    let mut command_line = new();
    up(&mut command_line);
    command_line.handle_action(key(KeyCode::Char('x')));
    assert_eq!(value(&command_line), "send bx");
    assert!(command_line.completion.candidates.is_empty());
    assert_eq!(up(&mut command_line), "send bx", "no line starts with it");
}

/// MAX-163: a line recalled from the history lists nothing, so Up and Down keep walking;
/// Tab lists its completion, and then they move in the list. Typed text with candidates
/// moves in the list at once.
/// How: Up on an empty line, Up again, Tab, Down; then "toggle-d" typed and Down.
#[test]
fn arrows_move_in_the_list_when_one_is_shown() {
    let history = vec!["help".into(), "toggle-d".into()];
    let mut command_line = CommandLine::new(history.clone(), Vec::new(), Vec::new());
    command_line.handle_action(key(KeyCode::Up));
    assert_eq!(value(&command_line), "toggle-d");
    assert!(command_line.completion.candidates.is_empty());
    command_line.handle_action(key(KeyCode::Up));
    assert_eq!(value(&command_line), "help");
    command_line.handle_action(key(KeyCode::Down));
    command_line.handle_action(DialogAction::CycleNext);
    assert_eq!(command_line.completion.candidates.len(), 3);
    command_line.handle_action(key(KeyCode::Down));
    assert_eq!(value(&command_line), "toggle-display-mode");

    let mut command_line = CommandLine::new(history, Vec::new(), Vec::new());
    command_line.handle_action(DialogAction::Paste("toggle-d".into()));
    command_line.handle_action(key(KeyCode::Down));
    assert_eq!(value(&command_line), "toggle-display-mode");
    command_line.handle_action(key(KeyCode::Up));
    assert_eq!(value(&command_line), "toggle-display-timestamp");
}

/// A folder taken is followed by no space: its entries are completed next
#[test]
fn take_a_folder() {
    let mut command_line = line("x ");
    command_line.completion = Completion {
        start: 2,
        candidates: vec![commands::Candidate {
            value: "logs/".to_string(),
            help: "",
        }],
    };
    command_line.handle_action(DialogAction::CycleNext);
    assert_eq!(value(&command_line), "x logs/");
}
