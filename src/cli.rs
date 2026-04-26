use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug, PartialEq, Eq)]
#[command(name = "ranchero", version, about = "Zwift live-data daemon")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, Debug, PartialEq, Eq, Default, Clone)]
pub struct GlobalOpts {
    #[arg(short = 'v', long, global = true, help = "Verbose output")]
    pub verbose: bool,

    #[arg(
        short = 'D',
        long,
        global = true,
        help = "Emit debug output (implies --foreground)"
    )]
    pub debug: bool,

    #[arg(long, global = true, help = "Stay in the foreground (do not daemonize)")]
    pub foreground: bool,

    #[arg(long, value_name = "EMAIL", global = true, help = "Override main account email")]
    pub mainuser: Option<String>,

    #[arg(
        long,
        value_name = "PASSWORD",
        global = true,
        help = "Override main account password (warning: visible in `ps`)"
    )]
    pub mainpassword: Option<String>,

    #[arg(long, value_name = "EMAIL", global = true, help = "Override monitor account email")]
    pub monitoruser: Option<String>,

    #[arg(
        long,
        value_name = "PASSWORD",
        global = true,
        help = "Override monitor account password (warning: visible in `ps`)"
    )]
    pub monitorpassword: Option<String>,

    #[arg(long, value_name = "PATH", global = true, help = "Alternate configuration file")]
    pub config: Option<PathBuf>,
}

impl GlobalOpts {
    pub fn finalize(&mut self) {
        if self.debug {
            self.foreground = true;
        }
    }
}

#[derive(Subcommand, Debug, PartialEq, Eq, Clone, Copy)]
pub enum Command {
    /// Open an interactive TUI to configure the application.
    Configure,
    /// Start ranchero listening on a Zwift stream.
    Start,
    /// Stop the currently-running ranchero process.
    Stop,
    /// Print statistics about the running ranchero process, or report shutdown.
    Status,
}

impl Command {
    fn name(self) -> &'static str {
        match self {
            Command::Configure => "configure",
            Command::Start => "start",
            Command::Stop => "stop",
            Command::Status => "status",
        }
    }
}

use std::process::ExitCode;

pub fn parse_from<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut cli = Cli::try_parse_from(args)?;
    cli.global.finalize();
    Ok(cli)
}

pub fn run(cli: Cli) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(cli.command.name().to_string());
    if cli.global.verbose {
        parts.push("verbose".to_string());
    }
    if cli.global.debug {
        parts.push("debug".to_string());
    }
    if cli.global.foreground {
        parts.push("foreground".to_string());
    }

    let mut out = if parts.len() == 1 {
        parts.remove(0)
    } else {
        let head = parts.remove(0);
        format!("{head} ({})", parts.join(", "))
    };

    if cli.global.verbose
        && (cli.global.mainpassword.is_some() || cli.global.monitorpassword.is_some())
    {
        out.push_str("\nwarning: passing passwords on the command line exposes them to `ps`");
    }

    out
}

/// Real dispatcher: routes subcommands to their actual implementations.
/// The stub `run()` above remains for the STEP 01 test suite.
pub fn dispatch(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    use crate::config::{self, OsEnv, ResolvedConfig, store::FileConfigStore};
    use crate::daemon;
    use crate::tui::{self, InMemoryKeyringStore};

    match cli.command {
        Command::Configure => {
            let config_path = cli.global.config.clone()
                .unwrap_or_else(config::default_config_path);
            let mut store = FileConfigStore::new(config_path);
            let mut keyring = InMemoryKeyringStore::default();
            let saved = tui::run_configure(&mut store, &mut keyring)
                .map_err(|e| format!("{e}"))?;
            if saved {
                println!("Configuration saved.");
            } else {
                println!("Configuration unchanged.");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Start | Command::Stop | Command::Status => {
            let file = config::load(cli.global.config.as_deref())?;
            let resolved = ResolvedConfig::resolve(&cli.global, &OsEnv, Some(file))?;
            match cli.command {
                Command::Start => Ok(daemon::start(&resolved, cli.global.foreground)?),
                Command::Stop => Ok(daemon::stop(&resolved)?),
                Command::Status => Ok(daemon::status(&resolved)?),
                Command::Configure => unreachable!(),
            }
        }
    }
}
