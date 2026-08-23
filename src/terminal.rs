use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Write, stdout};

pub struct TerminalGuard {
    cleanup_done: bool,
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, Hide)?;
        stdout.flush()?;

        Ok(Self {
            cleanup_done: false,
        })
    }

    pub fn cleanup(&mut self) {
        if self.cleanup_done {
            return;
        }

        self.cleanup_done = true;

        let mut stdout = stdout();
        let _ = execute!(stdout, Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        let _ = stdout.flush();
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.cleanup();
    }
}
