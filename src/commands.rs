#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Quit,
    Restart,
    Clear,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandError {
    NotFound,
}

pub fn parse_command(input: &str) -> Result<Command, CommandError> {
    match input.to_lowercase().trim() {
        "quit" | "q" | "exit" => Ok(Command::Quit),
        "restart" | "r" => Ok(Command::Restart),
        "clear" | "cls" => Ok(Command::Clear),
        _ => Err(CommandError::NotFound),
    }
}
