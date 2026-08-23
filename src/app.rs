use std::{
    collections::VecDeque,
    io::{Write, stdout},
};

use crossterm::execute;

pub struct App {
    /// Completed lines of device output (scrollback) String because already processed
    local_history: VecDeque<String>,
    /// Max line history
    max_lines: usize,
    /// Current line being received from device output as bytes
    partial_line: Vec<u8>,
    /// CUrrent line being typed by user as futur device input
    partial_user_line: String,
}

impl App {
    pub fn new(max_lines: usize) -> Self {
        Self {
            local_history: VecDeque::with_capacity(max_lines),
            max_lines,
            partial_line: Vec::new(),
            partial_user_line: String::new(),
        }
    }

    pub fn feed_rx(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.partial_line.push(b);

            if b == b'\n' {
                self.commit_partial_line();
            }
        }
    }

    fn commit_partial_line(&mut self) {
        // TODO: handle different formats, right now error symbol
        let line = String::from_utf8_lossy(&self.partial_line);

        let line = line.trim_end_matches(|c| c == '\r' || c == '\n');
        self.local_history.push_back(line.to_string());

        // Enforce maximum scrollback size.
        while self.local_history.len() > self.max_lines {
            self.local_history.pop_front();
        }

        self.partial_line.clear();
    }

    /// Append a character to the input line.
    pub fn append_input_char(&mut self, c: char) {
        self.partial_user_line.push(c);
    }

    /// Remove the last character from the input line.
    pub fn delete_last_input_char(&mut self) {
        self.partial_user_line.pop();
    }

    /// Commit the current input line (move it outside of self)
    pub fn commit_input_line(&mut self) -> String {
        std::mem::take(&mut self.partial_user_line)
    }

    pub fn render(&self) {
        let mut stdout = stdout();

        execute!(
            stdout,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0)
        )
        .unwrap();

        for line in &self.local_history {
            writeln!(stdout, "{line}").unwrap();
        }

        // TODO: make a config to wait for new line from the tty or display as it is coming
        // could be useful if the source is not emitting \n always
        if !self.partial_line.is_empty() {
            let partial = String::from_utf8_lossy(&self.partial_line);
            write!(stdout, "{partial}\r\n").unwrap();
        }

        write!(stdout, "> {}", self.partial_user_line).unwrap();

        stdout.flush().unwrap();
    }
}
