use std::process::ExitCode;

use ranchero::cli;

fn main() -> ExitCode {
    match cli::parse_from(std::env::args_os()) {
        Ok(parsed) => match cli::dispatch(parsed) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Err(err) => err.exit(),
    }
}
