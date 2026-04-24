use std::process::ExitCode;

use ranchero::cli;

fn main() -> ExitCode {
    match cli::parse_from(std::env::args_os()) {
        Ok(parsed) => {
            println!("{}", cli::run(parsed));
            ExitCode::SUCCESS
        }
        Err(err) => err.exit(),
    }
}
