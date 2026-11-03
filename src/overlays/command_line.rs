use std::cell::Cell;

use crossterm::event::KeyCode;
use tui_input::{Input, InputRequest};

use crate::{
    commands::{self, Completion},
    inputs::{DialogAction, click_input_at, dispatch_input_key, paste_into_input},
    overlays::{OverlayResponse, PopupTarget},
    ui::popups::command_line::COMPLETION_ROWS,
};

/// The command line (MAX-12): a `:` prompt whose line runs as commands on Enter. A line that
/// doesn't parse runs nothing and stays open to be fixed. Typed text has the completion of the
/// word before the cursor listed: Tab, Shift-Tab, Up and Down put a candidate in its place; a
/// lone one or a click takes it and moves on to the next word. An empty line, or one recalled
/// from the history, lists nothing until Tab or typing (MAX-163); without a list, Up and Down
/// walk the lines that ran starting with the text typed. The wheel scrolls the list (MAX-190)
#[derive(Debug, Clone, Default)]
pub struct CommandLine {
    pub input: Input,
    /// The text's columns scrolled out on the left in the last frame
    pub scroll: Cell<usize>,
    /// The completion of the word as typed, kept while its candidates are cycled
    pub completion: Completion,
    pub selected: Option<usize>,
    /// The first candidate shown: the wheel scrolls it, a selection out of view brings it back
    pub list_scroll: usize,
    /// Whether the completion is listed: it is empty while not
    listed: bool,
    /// The app's command history when it opened, the oldest first, the line shown from it, and
    /// the line typed before walking it, which the lines walked start with
    history: Vec<String>,
    history_index: Option<usize>,
    draft: String,
    /// The names of the triggers, to complete `trigger`'s (MAX-6), this machine's saved links,
    /// to complete `link-delete`'s and an address (MAX-185), and the serial ports listed when it
    /// opened, to complete an address (MAX-168)
    trigger_names: Vec<String>,
    link_names: Vec<String>,
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

    pub fn with_link_names(mut self, link_names: Vec<String>) -> Self {
        self.link_names = link_names;
        self
    }

    /// An older line (`up`) or a newer one from the history in place of the line, among the
    /// ones starting with the line typed before; past the newest, that line. Its completion
    /// isn't listed until Tab or typing
    fn walk_history(&mut self, up: bool) {
        if self.history_index.is_none() {
            if !up {
                return;
            }
            self.draft = self.input.value().to_string();
        }
        let matches = |i: &usize| self.history[*i].starts_with(&self.draft);
        let index = match (self.history_index, up) {
            (None, _) => (0..self.history.len()).rev().find(matches),
            (Some(i), true) => (0..i).rev().find(matches).or(Some(i)),
            (Some(i), false) => (i + 1..self.history.len()).find(matches),
        };
        if index.is_none() && self.history_index.is_none() {
            return;
        }
        let text = match index {
            Some(i) => self.history[i].clone(),
            None => std::mem::take(&mut self.draft),
        };
        self.history_index = index;
        self.input = Input::new(text);
        self.listed = false;
        self.refresh();
    }

    /// The completion of the word now before the cursor when listed, none selected
    fn refresh(&mut self) {
        let before_cursor = &self.input.value()[..self.cursor_byte()];
        self.completion = if self.listed {
            commands::complete(
                before_cursor,
                &self.trigger_names,
                &self.link_names,
                &self.ports,
            )
        } else {
            Completion::default()
        };
        self.selected = None;
        self.list_scroll = 0;
    }

    /// After an edit: the completion listed while there is text, the history walk over
    fn edited(&mut self) {
        self.history_index = None;
        self.listed = !self.input.value().is_empty();
        self.refresh();
    }

    /// The next candidate, or the list first when it isn't shown
    fn tab(&mut self, forward: bool) {
        if self.listed {
            self.cycle(forward);
        } else {
            self.listed = true;
            self.refresh();
        }
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
        let last_shown = self.list_scroll + COMPLETION_ROWS - 1;
        if selected < self.list_scroll {
            self.list_scroll = selected;
        } else if selected > last_shown {
            self.list_scroll = selected + 1 - COMPLETION_ROWS;
        }
    }

    /// The list one row down (`down`) or up, the last rows shown at most
    fn scroll(&mut self, down: bool) {
        let max = (self.completion.candidates.len()).saturating_sub(COMPLETION_ROWS);
        self.list_scroll = if down {
            (self.list_scroll + 1).min(max)
        } else {
            self.list_scroll.saturating_sub(1)
        };
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
                self.tab(true);
                OverlayResponse::Handled
            }
            DialogAction::CyclePrev => {
                self.tab(false);
                OverlayResponse::Handled
            }
            DialogAction::HandleKey(key) => {
                let listing = !self.completion.candidates.is_empty();
                match key.code {
                    KeyCode::Down if listing => self.cycle(true),
                    KeyCode::Up if listing => self.cycle(false),
                    KeyCode::Down => self.walk_history(false),
                    KeyCode::Up => self.walk_history(true),
                    _ => {
                        let before = self.input.clone();
                        dispatch_input_key(&mut self.input, key);
                        // a move of the cursor alone keeps the list as it is
                        if self.input.value() == before.value() {
                            self.refresh();
                        } else {
                            self.edited();
                        }
                    }
                }
                OverlayResponse::Handled
            }
            DialogAction::Paste(text) => {
                paste_into_input(&mut self.input, &text);
                self.edited();
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
            // the wheel (MAX-190): the arrows come as keys
            DialogAction::Down | DialogAction::Up => {
                self.scroll(action == DialogAction::Down);
                OverlayResponse::Handled
            }
            _ => OverlayResponse::Handled,
        }
    }
}

#[cfg(test)]
mod tests {
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
}
