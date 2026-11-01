use std::cell::Cell;

use crossterm::event::KeyCode;
use tui_input::{Input, InputRequest};

use crate::{
    commands::{self, Completion},
    inputs::{DialogAction, click_input_at, dispatch_input_key, paste_into_input},
    overlays::{OverlayResponse, PopupTarget},
};

/// The command line (MAX-12): a `:` prompt whose line runs as commands on Enter. A line that
/// doesn't parse runs nothing and stays open to be fixed. The word before the cursor has its
/// completion listed: Tab and Shift-Tab put a candidate in its place, then Up and Down too; a
/// lone one or a click takes it and moves on to the next word. Otherwise Up and Down walk the
/// lines that ran, as the TX input's history
#[derive(Debug, Clone, Default)]
pub struct CommandLine {
    pub input: Input,
    /// The text's columns scrolled out on the left in the last frame
    pub scroll: Cell<usize>,
    /// The completion of the word as typed, kept while its candidates are cycled
    pub completion: Completion,
    pub selected: Option<usize>,
    /// The app's command history when it opened, the oldest first, the line shown from it, and
    /// the line typed before walking it
    history: Vec<String>,
    history_index: Option<usize>,
    draft: String,
    /// The names of the triggers, to complete `trigger`'s (MAX-6), and the serial ports
    /// listed when it opened, to complete an address (MAX-168)
    trigger_names: Vec<String>,
    ports: Vec<String>,
}

impl CommandLine {
    pub fn new(history: Vec<String>, trigger_names: Vec<String>, ports: Vec<String>) -> Self {
        let mut command_line = Self {
            history,
            trigger_names,
            ports,
            ..Self::default()
        };
        command_line.refresh();
        command_line
    }

    /// An older line (`up`) or a newer one from the history in place of the line; past the
    /// newest, the line typed before
    fn walk_history(&mut self, up: bool) {
        let index = match (self.history_index, up) {
            (None, true) if !self.history.is_empty() => {
                self.draft = self.input.value().to_string();
                Some(self.history.len() - 1)
            }
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) if i + 1 < self.history.len() => Some(i + 1),
            (Some(_), false) => None,
            (None, _) => return,
        };
        let text = match index {
            Some(i) => self.history[i].clone(),
            None => std::mem::take(&mut self.draft),
        };
        self.history_index = index;
        self.input = Input::new(text);
        self.refresh();
    }

    /// The completion of the word now before the cursor, none selected
    fn refresh(&mut self) {
        let before_cursor = &self.input.value()[..self.cursor_byte()];
        self.completion = commands::complete(before_cursor, &self.trigger_names, &self.ports);
        self.selected = None;
    }

    fn cursor_byte(&self) -> usize {
        let value = self.input.value();
        value
            .char_indices()
            .nth(self.input.cursor())
            .map_or(value.len(), |(i, _)| i)
    }

    /// The next candidate (`forward`) or the previous one in the word's place, from the first
    /// or the last when none is selected. A lone candidate is taken
    fn cycle(&mut self, forward: bool) {
        let count = self.completion.candidates.len();
        if count == 1 {
            return self.take(0);
        }
        if count == 0 {
            return;
        }
        let selected = match (self.selected, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(i), true) => (i + 1) % count,
            (Some(i), false) => (i + count - 1) % count,
        };
        let value = self.completion.candidates[selected].value.clone();
        self.put_word(&value);
        self.selected = Some(selected);
    }

    /// The candidate in the word's place and a space after it, but for a folder (`/`): its
    /// entries come next. Then the next word's completion
    fn take(&mut self, index: usize) {
        let Some(candidate) = self.completion.candidates.get(index) else {
            return;
        };
        let value = candidate.value.clone();
        self.put_word(&value);
        let after = &self.input.value()[self.cursor_byte()..];
        if !value.ends_with('/') && !after.starts_with(char::is_whitespace) {
            self.input.handle(InputRequest::InsertChar(' '));
        }
        self.refresh();
    }

    /// `word` in place of the word before the cursor, the cursor after it
    fn put_word(&mut self, word: &str) {
        let value = self.input.value();
        let start = self.completion.start;
        let line = format!("{}{word}{}", &value[..start], &value[self.cursor_byte()..]);
        let cursor = value[..start].chars().count() + word.chars().count();
        self.input = Input::new(line).with_cursor(cursor);
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        match action {
            DialogAction::Close => OverlayResponse::Close,
            DialogAction::Confirm => match commands::parse(self.input.value()) {
                Ok(invocations) if invocations.is_empty() => OverlayResponse::Close,
                Ok(invocations) => OverlayResponse::QueueCommands {
                    line: self.input.value().to_string(),
                    invocations,
                },
                Err(err) => OverlayResponse::HandledNotify {
                    message: err.to_string(),
                    warn: true,
                },
            },
            DialogAction::CycleNext => {
                self.cycle(true);
                OverlayResponse::Handled
            }
            DialogAction::CyclePrev => {
                self.cycle(false);
                OverlayResponse::Handled
            }
            DialogAction::HandleKey(key) => {
                let cycling = self.selected.is_some();
                match key.code {
                    KeyCode::Down if cycling => self.cycle(true),
                    KeyCode::Up if cycling => self.cycle(false),
                    KeyCode::Down => self.walk_history(false),
                    KeyCode::Up => self.walk_history(true),
                    _ => {
                        dispatch_input_key(&mut self.input, key);
                        self.history_index = None;
                        self.refresh();
                    }
                }
                OverlayResponse::Handled
            }
            DialogAction::Paste(text) => {
                paste_into_input(&mut self.input, &text);
                self.history_index = None;
                self.refresh();
                OverlayResponse::Handled
            }
            DialogAction::ClickAt(column) => {
                click_input_at(&mut self.input, column);
                self.refresh();
                OverlayResponse::Handled
            }
            DialogAction::Pick(PopupTarget::Completion(index)) => {
                self.take(index);
                OverlayResponse::Handled
            }
            _ => OverlayResponse::Handled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> CommandLine {
        let mut command_line = CommandLine {
            input: Input::new(text.to_string()),
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

    /// The command line opens with every command listed
    #[test]
    fn opens_with_every_command() {
        let command_line = CommandLine::new(Vec::new(), Vec::new(), Vec::new());
        assert_eq!(
            command_line.completion.candidates.len(),
            commands::COMMANDS.len()
        );
        assert_eq!(command_line.selected, None);
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
    /// it; an edit starts from the edited line
    #[test]
    fn walk_the_history() {
        let mut command_line = CommandLine::new(
            vec!["help".into(), "toggle-mouse".into()],
            Vec::new(),
            Vec::new(),
        );
        command_line.handle_action(DialogAction::Paste("zz".into()));
        let up = |command_line: &mut CommandLine| {
            command_line.handle_action(key(KeyCode::Up));
            command_line.input.value().to_string()
        };
        let down = |command_line: &mut CommandLine| {
            command_line.handle_action(key(KeyCode::Down));
            command_line.input.value().to_string()
        };
        assert_eq!(up(&mut command_line), "toggle-mouse");
        assert_eq!(up(&mut command_line), "help");
        assert_eq!(up(&mut command_line), "help");
        assert_eq!(down(&mut command_line), "toggle-mouse");
        assert_eq!(down(&mut command_line), "zz");
        assert_eq!(down(&mut command_line), "zz");

        up(&mut command_line);
        command_line.handle_action(key(KeyCode::Char('x')));
        assert_eq!(up(&mut command_line), "toggle-mouse");
        assert_eq!(down(&mut command_line), "toggle-mousex");
    }

    /// With the completion listed but none selected, Up walks the history; once Tab selected a
    /// candidate, Up and Down move in the list
    #[test]
    fn arrows_walk_the_history_until_tab() {
        let mut command_line = CommandLine::new(vec!["help".into()], Vec::new(), Vec::new());
        assert!(!command_line.completion.candidates.is_empty());
        command_line.handle_action(key(KeyCode::Up));
        assert_eq!(value(&command_line), "help");

        let mut command_line = CommandLine::new(vec!["help".into()], Vec::new(), Vec::new());
        command_line.handle_action(DialogAction::Paste("toggle-d".into()));
        command_line.handle_action(DialogAction::CycleNext);
        command_line.handle_action(key(KeyCode::Down));
        assert_eq!(value(&command_line), "toggle-display-raw");
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
}
