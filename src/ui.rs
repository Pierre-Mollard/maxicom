/// View, render screen from state (app)
/// Using the '??tui??' crate
use std::io::Write;

use crossterm::execute;

use crate::app::App;

pub fn render<W: Write>(app: &App, out: &mut W) -> std::io::Result<()> {
    execute!(
        out,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
        crossterm::cursor::MoveTo(0, 0)
    )?;

    for line in app.history() {
        write!(out, "| {line}\r\n")?;
    }

    if app.display_partial_incoming() && !app.partial_rx().is_empty() {
        let partial = String::from_utf8_lossy(app.partial_rx());
        write!(out, "|p {partial}\r\n")?;
    }

    write!(out, "> {}", app.input_buffer())?;
    out.flush()?;
    Ok(())
}
