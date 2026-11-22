//! The `rsdroid` command.
//!
//! M0 ships `doctor` only. The command is a thin client: checks live in
//! `rsdroid-core`, and future management calls will go through `rsdroid-ipc`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use rsdroid_core::doctor::{BundleOptions, DoctorOptions};

#[derive(Debug, Parser)]
#[command(
    name = "rsdroid",
    version,
    about = "Open-source, ad-free, multi-window Android emulator for Windows",
    long_about = "rsdroid runs Android in QEMU with WHPX and streams it through scrcpy-server.\n\
                  M0 ships only the environment doctor; VM and manager commands arrive in M1-M3."
)]
struct Cli {
    /// Print machine-readable JSON on stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Suppress the human-readable report; errors and bundle paths still print.
    #[arg(long, global = true)]
    quiet: bool,

    /// Timeout for external version probes, in seconds.
    #[arg(
        long,
        global = true,
        default_value_t = 10,
        value_parser = clap::value_parser!(u64).range(1..=600)
    )]
    timeout: u64,

    /// Override the per-user data directory (default: %LOCALAPPDATA%\rsdroid).
    #[arg(long, global = true, env = "RSDROID_DATA_DIR", value_name = "DIR")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check whether this machine can run rsdroid.
    Doctor(DoctorArgs),
}

#[derive(Debug, Args)]
struct DoctorArgs {
    /// Write a ZIP with the report and existing logs. Nothing is uploaded.
    #[arg(long)]
    bundle: bool,

    /// Directory that receives the bundle (default: current directory).
    #[arg(long, value_name = "DIR", requires = "bundle")]
    bundle_dir: Option<PathBuf>,

    /// Explicit path to qemu-system-x86_64.exe.
    #[arg(long, value_name = "PATH")]
    qemu: Option<PathBuf>,

    /// Explicit path to adb.exe.
    #[arg(long, value_name = "PATH")]
    adb: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    /// A core operation failed.
    #[error(transparent)]
    Core(#[from] rsdroid_core::Error),
    /// The report could not be serialized.
    #[error("cannot serialize the report: {0}")]
    Json(#[from] serde_json::Error),
    /// The current directory is unavailable.
    #[error("cannot determine the current directory: {0}")]
    CurrentDir(#[from] std::io::Error),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.quiet);
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("rsdroid: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<u8, CliError> {
    match &cli.command {
        Command::Doctor(args) => doctor(&cli, args),
    }
}

fn doctor(cli: &Cli, args: &DoctorArgs) -> Result<u8, CliError> {
    let options = DoctorOptions {
        qemu: args.qemu.clone(),
        adb: args.adb.clone(),
        data_dir: cli.data_dir.clone(),
        timeout: Duration::from_secs(cli.timeout),
    };
    let report = rsdroid_core::doctor::run(&options);

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if cli.quiet {
        println!(
            "doctor: {} ok, {} warning(s), {} error(s), {} skipped",
            report.summary.ok,
            report.summary.warnings,
            report.summary.errors,
            report.summary.skipped
        );
        for check in report
            .checks
            .iter()
            .filter(|check| check.status.is_blocking())
        {
            println!("  fail: {}: {}", check.title, check.detail);
        }
    } else {
        print!("{}", report.render_human());
    }

    if args.bundle {
        let output_dir = match &args.bundle_dir {
            Some(directory) => directory.clone(),
            None => std::env::current_dir()?,
        };
        let data_dir = cli
            .data_dir
            .clone()
            .or_else(|| rsdroid_core::paths::data_dir().ok());
        let bundle =
            rsdroid_core::doctor::write_bundle(&report, &BundleOptions::new(output_dir, data_dir))?;
        // Keep stdout valid JSON when --json is used; the path is a diagnostic.
        eprintln!("doctor bundle: {}", bundle.display());
        if !cli.json && !cli.quiet {
            println!("\nBundle written to {}", bundle.display());
        }
    }

    Ok(report.exit_code())
}

fn init_logging(quiet: bool) {
    let default_level = if quiet { "error" } else { "warn" };
    let filter = tracing_subscriber::EnvFilter::try_from_env("RSDROID_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(false)
        .try_init();
}
