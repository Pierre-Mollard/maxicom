/* TODO: unused, for later with command line input (not tty!)


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

*/
/* BROKEN
*
        let result = commands::parse_command(&input_buffer);
        match result {
            Ok(commands::Command::Quit) => {
                running = false;
            }

            Ok(commands::Command::Restart) => {
                println!("Restart is not implemented yet.");
            }

            Ok(commands::Command::Clear) => {
                println!("Clear is not implemented yet.");
            }

            Err(commands::CommandError::NotFound) => {
                println!("Unknown command: {}", input_buffer.trim());
            }
        }
*/
