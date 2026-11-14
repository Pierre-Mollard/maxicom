use std::cell::Cell;

use crossterm::event::KeyCode;
use tui_input::{Input, InputRequest};

use crate::{
    commands::{self, Completion, NamedCommand},
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
    /// The plots of `plots.toml`, to complete `plot`'s (MAX-222)
    plot_names: Vec<String>,
    /// The commands of `[commands]` in `keymap.toml`, run and completed by name (MAX-239)
    named: Vec<NamedCommand>,
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

    pub fn with_plot_names(mut self, plot_names: Vec<String>) -> Self {
        self.plot_names = plot_names;
        self
    }

    pub fn with_named_commands(mut self, named: Vec<NamedCommand>) -> Self {
        self.named = named;
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
                &self.plot_names,
                &self.named,
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
            DialogAction::Confirm => match commands::parse_with(self.input.value(), &self.named) {
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
#[path = "../tests/overlays/command_line.rs"]
mod tests;
