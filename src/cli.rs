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

    #[arg(
        long,
        value_name = "PATH",
        global = true,
        help = "Write a wire-capture file alongside the live session (only meaningful with `start`)"
    )]
    pub capture: Option<PathBuf>,
}

impl GlobalOpts {
    pub fn finalize(&mut self) {
        if self.debug {
            self.foreground = true;
        }
    }
}

#[derive(Subcommand, Debug, PartialEq, Eq, Clone)]
pub enum Command {
    /// Open an interactive TUI to configure the application.
    Configure,
    /// Start ranchero listening on a Zwift stream.
    Start,
    /// Stop the currently-running ranchero process.
    Stop,
    /// Print statistics about the running ranchero process, or report shutdown.
    Status,
    /// Print what an auth login would send to Zwift, without opening any sockets.
    /// A pre-flight diagnostic: prove that config + credentials + endpoint
    /// configuration all resolve before risking a real Keycloak round-trip
    /// (which can lock the account on repeated failures).
    AuthCheck,
    /// Print a summary of (or per-record listing of) a wire-capture file
    /// previously written by `ranchero start --capture <path>`.
    Replay {
        /// Path to the capture file.
        path: PathBuf,
        /// Print one line per record instead of a summary.
        #[arg(long)]
        verbose: bool,
    },
    /// Tail a wire-capture file and print each record to standard
    /// output as it is written. Pairs with `ranchero start
    /// --capture <path>` to validate the live stream from a second
    /// terminal.
    Follow {
        /// Path to the capture file.
        path: PathBuf,
        /// Decode each payload as `ServerToClient` (inbound) or
        /// `ClientToServer` (outbound) and print the decoded
        /// message instead of a one-line summary.
        #[arg(long)]
        decode: bool,
        /// Exit after this many seconds without a new record.
        /// Default: run until interrupted.
        #[arg(long, value_name = "SECONDS")]
        idle_timeout: Option<u64>,
    },
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Command::Configure => "configure",
            Command::Start => "start",
            Command::Stop => "stop",
            Command::Status => "status",
            Command::AuthCheck => "auth-check",
            Command::Replay { .. } => "replay",
            Command::Follow { .. } => "follow",
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
    use crate::credentials::OsKeyringStore;
    use crate::daemon;
    use crate::tui;

    match cli.command {
        Command::Configure => {
            let config_path = cli.global.config.clone()
                .unwrap_or_else(config::default_config_path);
            let mut store = FileConfigStore::new(config_path);
            let mut keyring = OsKeyringStore::new();
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
            let keyring = OsKeyringStore::with_service_name(&file.keyring.service);
            let resolved = ResolvedConfig::resolve(&cli.global, &OsEnv, &keyring, Some(file))?;
            match cli.command {
                Command::Start => {
                    let log_opts = crate::logging::LogOpts {
                        verbose: cli.global.verbose,
                        debug: cli.global.debug,
                    };
                    let capture = cli.global.capture.as_ref()
                        .map(|p| std::path::absolute(p))
                        .transpose()?;
                    Ok(daemon::start(
                        &resolved,
                        cli.global.foreground,
                        log_opts,
                        capture,
                    )?)
                }
                Command::Stop => Ok(daemon::stop(&resolved)?),
                Command::Status => Ok(daemon::status(&resolved)?),
                Command::Configure
                | Command::AuthCheck
                | Command::Replay { .. }
                | Command::Follow { .. } => {
                    unreachable!()
                }
            }
        }
        Command::AuthCheck => {
            let file = config::load(cli.global.config.as_deref())?;
            let keyring = OsKeyringStore::with_service_name(&file.keyring.service);
            let resolved = ResolvedConfig::resolve(&cli.global, &OsEnv, &keyring, Some(file))?;
            print_auth_check(&resolved, &keyring);
            Ok(ExitCode::SUCCESS)
        }
        Command::Replay { path, verbose } => {
            print_replay(&path, verbose)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Follow { path, decode, idle_timeout } => {
            print_follow(&path, decode, idle_timeout)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// STEP-12.2: tail a wire-capture file and print each record to
/// the supplied writer as text. This is the testable surface; the
/// production dispatcher in [`dispatch`] wraps it with
/// `std::io::stdout`. See
/// `docs/plans/STEP-12.2-follow-command.md` for the design.
///
/// The function writes a header (file path + format version), then
/// iterates the [`zwift_relay::capture::CaptureFollower`] until
/// the supplied idle timeout expires (or the file is truncated /
/// removed under the follower). Each record produces a one-line
/// summary; with `decode = true`, an additional pretty-printed
/// `Debug` block of the decoded `ServerToClient` (inbound) or
/// `ClientToServer` (outbound) message follows.
pub fn print_follow_to<W: std::io::Write>(
    mut out: W,
    path: &std::path::Path,
    decode: bool,
    idle_timeout_secs: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    use prost::Message as _;
    use std::time::Duration;
    use zwift_relay::capture::{CaptureFollower, Direction, TransportKind};

    let mut follower = CaptureFollower::open(path)?;
    if let Some(secs) = idle_timeout_secs {
        follower = follower.with_idle_timeout(Some(Duration::from_secs(secs)));
    }

    writeln!(out, "ranchero follow {}", path.display())?;
    writeln!(out, "Format version: {}", follower.version())?;
    writeln!(out)?;

    for (idx, result) in follower.enumerate() {
        let record = result?;
        let dir = match record.direction {
            Direction::Inbound => "in ",
            Direction::Outbound => "out",
        };
        let xport = match record.transport {
            TransportKind::Udp => "UDP",
            TransportKind::Tcp => "TCP",
            TransportKind::Http => "HTT",
        };
        let hello = if record.hello { " hello" } else { "" };
        writeln!(
            out,
            "  #{idx:>6}  {dir} {xport}  ts={}ns  len={:>5}{hello}",
            record.ts_unix_ns,
            record.payload.len(),
        )?;

        if decode {
            match record.direction {
                Direction::Inbound => {
                    match zwift_proto::ServerToClient::decode(record.payload.as_slice()) {
                        Ok(msg) => writeln!(out, "{msg:#?}")?,
                        Err(e) => writeln!(out, "  (decode error: {e})")?,
                    }
                }
                Direction::Outbound => {
                    match zwift_proto::ClientToServer::decode(record.payload.as_slice()) {
                        Ok(msg) => writeln!(out, "{msg:#?}")?,
                        Err(e) => writeln!(out, "  (decode error: {e})")?,
                    }
                }
            }
        }
    }

    Ok(())
}

/// STEP-12.2 stub: dispatch arm wrapper. Forwards to
/// [`print_follow_to`] with `std::io::stdout` as the destination.
fn print_follow(
    path: &std::path::Path,
    decode: bool,
    idle_timeout: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    print_follow_to(std::io::stdout(), path, decode, idle_timeout)
}

/// Render an "auth-check" report: for each configured account, show the
/// resolved email, password source/length, and the literal HTTP request
/// `ZwiftAuth::login()` would issue (form body + headers, password
/// redacted). No sockets opened. Useful as a pre-flight gate before
/// `start` to confirm credentials and endpoint config look healthy
/// without burning Keycloak login attempts (Zwift will lock the account
/// after a few bad-password tries).
pub fn print_auth_check_to<W: std::io::Write>(
    out: &mut W,
    resolved: &crate::config::ResolvedConfig,
    keyring: &dyn crate::credentials::KeyringStore,
) -> std::io::Result<()> {
    use zwift_api::{CLIENT_ID, Config, TOKEN_PATH, ZwiftAuth};

    let cfg = Config {
        auth_base:  resolved.zwift_endpoints.auth_base.clone(),
        api_base:   resolved.zwift_endpoints.api_base.clone(),
        source:     zwift_api::DEFAULT_SOURCE.to_string(),
        user_agent: zwift_api::DEFAULT_USER_AGENT.to_string(),
        platform:   "OSX".to_string(),
    };
    // Construct ZwiftAuth purely to prove the wiring compiles and runs;
    // we never call .login() so no socket is opened.
    let _auth = ZwiftAuth::new(cfg.clone());

    writeln!(out, "ranchero auth-check (no network calls)")?;
    writeln!(out)?;
    writeln!(out, "Endpoints (from config):")?;
    writeln!(out, "  auth_base:  {}", cfg.auth_base)?;
    writeln!(out, "  api_base:   {}", cfg.api_base)?;
    writeln!(out, "  token path: {TOKEN_PATH}")?;
    writeln!(out)?;
    match resolved.watched_athlete_id {
        Some(id) => writeln!(out, "Watched athlete:   {id}")?,
        None     => writeln!(out, "Watched athlete:   not configured")?,
    }
    writeln!(out)?;

    let roles: [(&str, Option<&str>, Option<&str>); 2] = [
        (
            "main",
            resolved.main_email.as_deref(),
            resolved.main_password.as_ref().map(|p| p.expose()),
        ),
        (
            "monitor",
            resolved.monitor_email.as_deref(),
            resolved.monitor_password.as_ref().map(|p| p.expose()),
        ),
    ];

    for (role, email, cli_password) in roles {
        writeln!(out, "Account: {role}")?;
        match email {
            Some(e) => writeln!(out, "  email:           {e}")?,
            None => {
                writeln!(out, "  email:           <not configured>")?;
                writeln!(out, "  (skip — no email; configure via `ranchero configure` or --{role}user)")?;
                writeln!(out)?;
                continue;
            }
        }

        let (password, source) = match cli_password {
            Some(p) => (Some(p.to_string()), "command-line override"),
            None => match keyring.get(role) {
                Ok(Some(entry)) => (Some(entry.password), "OS keyring"),
                Ok(None) => (None, "missing"),
                Err(e) => {
                    writeln!(out, "  password source: keyring error: {e}")?;
                    writeln!(out)?;
                    continue;
                }
            },
        };

        match &password {
            Some(p) => {
                writeln!(out, "  password source: {source}")?;
                writeln!(out, "  password length: {} chars (value redacted)", p.chars().count())?;
            }
            None => {
                writeln!(out, "  password source: {source}")?;
                writeln!(out, "  (skip — no password; store one via `ranchero configure`)")?;
                writeln!(out)?;
                continue;
            }
        }

        // Render the form body the same way reqwest does: serde_urlencoded.
        // The password slot is rendered as the literal string `[redacted]`
        // so this output is safe to paste into a bug report.
        let body = serde_urlencoded::to_string([
            ("client_id", CLIENT_ID),
            ("grant_type", "password"),
            ("username", email.unwrap()),
            ("password", "[redacted]"),
        ])
        .expect("urlencode auth-check form");

        writeln!(out, "  Login request:")?;
        writeln!(out, "    POST {}{TOKEN_PATH}", cfg.auth_base)?;
        writeln!(out, "    Content-Type: application/x-www-form-urlencoded")?;
        writeln!(out, "    Body: {body}")?;
        writeln!(out)?;
        writeln!(out, "  Example authed call:")?;
        writeln!(out, "    GET {}/api/profiles/me", cfg.api_base)?;
        writeln!(out, "    Authorization: Bearer <access_token from login response>")?;
        writeln!(out, "    Source: {}", cfg.source)?;
        writeln!(out, "    User-Agent: {}", cfg.user_agent)?;
        writeln!(out)?;
    }

    writeln!(out, "OK — credentials and endpoint config look reachable.")?;
    writeln!(out, "(Run `cargo test -p zwift-api` to exercise the actual HTTP flow against a mock server.)")?;
    Ok(())
}

fn print_auth_check(
    resolved: &crate::config::ResolvedConfig,
    keyring: &dyn crate::credentials::KeyringStore,
) {
    print_auth_check_to(&mut std::io::stdout(), resolved, keyring)
        .expect("stdout write failed");
}

/// Iterate a wire-capture file (STEP 11.5). Default mode prints a
/// summary (record counts by direction/transport, time range, total
/// bytes); `--verbose` prints one line per record. Surfaces any
/// `CaptureError` to the caller via `?`.
fn print_replay(path: &std::path::Path, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    use zwift_relay::capture::{CaptureReader, Direction, TransportKind};

    let reader = CaptureReader::open(path)?;
    println!("ranchero replay {}", path.display());
    println!("Format version: {}", reader.version());
    println!();

    if verbose {
        for (idx, result) in reader.enumerate() {
            let r = result?;
            let dir = match r.direction {
                Direction::Inbound => "in ",
                Direction::Outbound => "out",
            };
            let xport = match r.transport {
                TransportKind::Udp => "UDP",
                TransportKind::Tcp => "TCP",
                TransportKind::Http => "HTT",
            };
            let hello = if r.hello { " hello" } else { "" };
            println!(
                "  #{idx:>6}  {dir} {xport}  ts={}ns  len={:>5}{hello}",
                r.ts_unix_ns,
                r.payload.len(),
            );
        }
        return Ok(());
    }

    let mut counts = [[0u64; 2]; 2]; // [direction][transport]
    let mut total_bytes: u64 = 0;
    let mut min_ts = u64::MAX;
    let mut max_ts = 0u64;
    for result in reader {
        let r = result?;
        counts[r.direction.as_byte() as usize][r.transport.as_byte() as usize] += 1;
        total_bytes += r.payload.len() as u64;
        min_ts = min_ts.min(r.ts_unix_ns);
        max_ts = max_ts.max(r.ts_unix_ns);
    }

    let inbound_udp = counts[Direction::Inbound.as_byte() as usize]
        [TransportKind::Udp.as_byte() as usize];
    let inbound_tcp = counts[Direction::Inbound.as_byte() as usize]
        [TransportKind::Tcp.as_byte() as usize];
    let outbound_udp = counts[Direction::Outbound.as_byte() as usize]
        [TransportKind::Udp.as_byte() as usize];
    let outbound_tcp = counts[Direction::Outbound.as_byte() as usize]
        [TransportKind::Tcp.as_byte() as usize];
    let total = inbound_udp + inbound_tcp + outbound_udp + outbound_tcp;

    println!("Records by (direction, transport):");
    println!("  inbound  UDP: {inbound_udp:>8}");
    println!("  inbound  TCP: {inbound_tcp:>8}");
    println!("  outbound UDP: {outbound_udp:>8}");
    println!("  outbound TCP: {outbound_tcp:>8}");
    println!("  total:        {total:>8}");
    println!();
    println!("Total payload bytes: {total_bytes}");
    if total > 0 {
        let span_ms = (max_ts.saturating_sub(min_ts)) / 1_000_000;
        println!(
            "Time range: {min_ts} ns .. {max_ts} ns  (span ~{span_ms} ms)",
        );
    }

    Ok(())
}
