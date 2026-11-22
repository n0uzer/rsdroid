//! The `rsdroid` command.
//!
//! M0 ships `doctor`. M1 adds `run` and `stop`, which launch one headless VM and
//! control it over loopback QMP. The manager API and IPC arrive in M3.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use rsdroid_adb::{AdbClient, AdbError};
use rsdroid_core::doctor::{BundleOptions, DoctorOptions};
use rsdroid_vm::config::{
    run_dir, DEFAULT_ADB_PORT, DEFAULT_CPUS, DEFAULT_CPU_MODEL, DEFAULT_MEMORY_MB,
    DEFAULT_QMP_PORT, DEFAULT_SERIAL_PORT,
};
use rsdroid_vm::state::{process_alive, read_state, remove_state, state_path, write_state};
use rsdroid_vm::{DisplayMode, QmpClient, QmpError, VmConfig, VmError, VmProcess, VmState};

#[derive(Debug, Parser)]
#[command(
    name = "rsdroid",
    version,
    about = "Open-source, ad-free, multi-window Android emulator for Windows",
    long_about = "rsdroid runs Android in QEMU with WHPX and streams it through scrcpy-server.\n\
                  M1 boots one headless Android-x86 VM from the pinned ISO and can stop it cleanly."
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
    /// Boot one headless VM and wait until Android reports boot complete.
    Run(RunArgs),
    /// Ask a running VM to power down through QMP.
    Stop(StopArgs),
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

#[derive(Debug, Args)]
struct RunArgs {
    /// Instance name (letters, digits, '-' and '_').
    instance: String,

    /// Android-x86 ISO (default: %LOCALAPPDATA%\rsdroid\images\android-x86_64-9.0-r2.iso).
    #[arg(long, value_name = "PATH")]
    iso: Option<PathBuf>,

    /// Kernel extracted from the ISO (default: next to the ISO in a matching folder).
    #[arg(long, value_name = "PATH")]
    kernel: Option<PathBuf>,

    /// Initrd extracted from the ISO (default: next to the kernel).
    #[arg(long, value_name = "PATH")]
    initrd: Option<PathBuf>,

    /// Explicit path to qemu-system-x86_64.exe.
    #[arg(long, value_name = "PATH")]
    qemu: Option<PathBuf>,

    /// Explicit path to adb.exe.
    #[arg(long, value_name = "PATH")]
    adb: Option<PathBuf>,

    /// Virtual CPUs.
    #[arg(long, default_value_t = DEFAULT_CPUS, value_parser = clap::value_parser!(u8).range(1..=64))]
    cpus: u8,

    /// Guest RAM in MiB.
    #[arg(long, default_value_t = DEFAULT_MEMORY_MB, value_parser = clap::value_parser!(u32).range(512..=65536))]
    memory_mb: u32,

    /// QEMU CPU model.
    #[arg(long, default_value = DEFAULT_CPU_MODEL)]
    cpu_model: String,

    /// Host loopback port forwarded to guest ADB port 5555.
    #[arg(long, default_value_t = DEFAULT_ADB_PORT, value_parser = clap::value_parser!(u16).range(1..=65535))]
    adb_port: u16,

    /// Host loopback port for QMP.
    #[arg(long, default_value_t = DEFAULT_QMP_PORT, value_parser = clap::value_parser!(u16).range(1..=65535))]
    qmp_port: u16,

    /// Host loopback port for the guest serial console.
    #[arg(long, default_value_t = DEFAULT_SERIAL_PORT, value_parser = clap::value_parser!(u16).range(1..=65535))]
    serial_port: u16,

    /// Show QEMU's own SDL window for troubleshooting.
    #[arg(long)]
    debug_window: bool,

    /// Give up waiting for sys.boot_completed after this many seconds.
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(1..=3600))]
    boot_timeout: u64,

    /// Print the QEMU command line and exit without starting anything.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct StopArgs {
    /// Instance name.
    instance: String,

    /// Seconds to wait for a clean guest powerdown before sending QMP quit.
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=600))]
    timeout: u64,
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    /// A core operation failed.
    #[error(transparent)]
    Core(#[from] rsdroid_core::Error),
    /// The report could not be serialized.
    #[error("cannot serialize JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The current directory is unavailable.
    #[error("cannot determine the current directory: {0}")]
    CurrentDir(#[from] std::io::Error),
    /// A required tool or file is missing or unusable.
    #[error("environment problem: {0}")]
    Environment(String),
    /// The instance has no state file.
    #[error("instance '{0}' is not running")]
    InstanceNotFound(String),
    /// The instance exists but QEMU is gone.
    #[error("instance '{0}' is not running (stale state removed)")]
    NotRunning(String),
    /// A loopback port is already in use.
    #[error("port {port} is already in use: {source}")]
    Port {
        /// Port that could not be bound.
        port: u16,
        /// Underlying socket error.
        #[source]
        source: std::io::Error,
    },
    /// A VM operation failed.
    #[error(transparent)]
    Vm(#[from] VmError),
    /// An ADB operation failed.
    #[error(transparent)]
    Adb(#[from] AdbError),
    /// A QMP operation failed.
    #[error(transparent)]
    Qmp(#[from] QmpError),
    /// The Ctrl+C handler could not be installed.
    #[error("cannot install Ctrl+C handler: {0}")]
    Signal(String),
    /// Another unrecoverable error.
    #[error("{0}")]
    General(String),
}

impl CliError {
    fn exit_code(&self) -> u8 {
        match self {
            Self::Environment(_) => 5,
            Self::InstanceNotFound(_) => 3,
            Self::Vm(VmError::Config(_)) => 5,
            Self::Vm(VmError::StartupTimeout(_)) | Self::Adb(AdbError::BootTimeout { .. }) => 4,
            _ => 1,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.quiet);
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("rsdroid: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}

fn run(cli: Cli) -> Result<u8, CliError> {
    match &cli.command {
        Command::Doctor(args) => doctor(&cli, args),
        Command::Run(args) => run_instance(&cli, args),
        Command::Stop(args) => stop_instance(&cli, args),
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

fn run_instance(cli: &Cli, args: &RunArgs) -> Result<u8, CliError> {
    let data_dir = resolve_data_dir(cli)?;
    let run_directory = run_dir(&data_dir, &args.instance)?;
    let iso = args
        .iso
        .clone()
        .unwrap_or_else(|| data_dir.join("images").join("android-x86_64-9.0-r2.iso"));
    let boot_dir = iso
        .parent()
        .map(|parent| {
            parent.join(
                iso.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        })
        .unwrap_or_else(|| data_dir.join("images"));
    let kernel = args
        .kernel
        .clone()
        .unwrap_or_else(|| boot_dir.join("kernel"));
    let initrd = args
        .initrd
        .clone()
        .unwrap_or_else(|| boot_dir.join("initrd.img"));

    let qemu = rsdroid_core::tools::find_qemu(args.qemu.as_deref()).ok_or_else(|| {
        CliError::Environment(
            "qemu-system-x86_64.exe was not found; install the pinned QEMU 11.1.0 build or pass --qemu <path>"
                .to_owned(),
        )
    })?;
    let adb = rsdroid_core::tools::find_adb(args.adb.as_deref()).ok_or_else(|| {
        CliError::Environment(
            "adb.exe was not found; install Android platform-tools or pass --adb <path>".to_owned(),
        )
    })?;

    if !iso.is_file() {
        return Err(CliError::Environment(format!(
            "Android-x86 ISO not found at {}; run 'pwsh tools/fetch-deps.ps1 -Component android-x86' or pass --iso <path>",
            iso.display()
        )));
    }
    if !kernel.is_file() || !initrd.is_file() {
        return Err(CliError::Environment(format!(
            "kernel/initrd not found in {}; run 'pwsh tools/extract-boot-files.ps1' or pass --kernel/--initrd",
            boot_dir.display()
        )));
    }

    let config = VmConfig {
        name: args.instance.clone(),
        qemu,
        kernel,
        initrd,
        iso,
        cpus: args.cpus,
        memory_mb: args.memory_mb,
        cpu_model: args.cpu_model.clone(),
        adb_port: args.adb_port,
        qmp_port: args.qmp_port,
        serial_port: args.serial_port,
        display: if args.debug_window {
            DisplayMode::Window
        } else {
            DisplayMode::Headless
        },
        pid_file: run_directory.join("qemu.pid"),
        log_file: run_directory.join("qemu.log"),
    };

    if args.dry_run {
        let rendered = rsdroid_vm::command::render(&config);
        if cli.json {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "instance": config.name,
                    "command": rendered,
                    "adb_serial": rsdroid_vm::command::adb_serial(&config),
                    "qmp_port": config.qmp_port,
                }))?
            );
        } else {
            println!("{rendered}");
        }
        return Ok(0);
    }

    config.validate()?;
    for port in [config.adb_port, config.qmp_port, config.serial_port] {
        ensure_port_free(port)?;
    }

    let mut process = VmProcess::start(&config)?;
    let _cleanup = StateCleanup {
        run_dir: run_directory,
    };
    let state = VmState::new(&config, process.pid());
    let state_file = write_state(&state_file_dir(&config), &state)?;
    let mut qmp = process.connect_qmp(config.qmp_port, Duration::from_secs(10))?;
    let status = qmp.query_status()?;
    tracing::debug!(status = %status.status, "QMP connected");

    let adb_client = AdbClient::new(adb, config.adb_port);
    let boot_seconds = match adb_client.wait_for_boot(
        Duration::from_secs(args.boot_timeout),
        Duration::from_secs(1),
    ) {
        Ok(seconds) => seconds,
        Err(error) => {
            abort_vm(&mut process, &mut qmp);
            return Err(error.into());
        }
    };

    // QEMU accepts one QMP client at a time. Release the connection now so
    // `rsdroid stop` can attach; reconnect on Ctrl+C for the clean shutdown.
    drop(qmp);

    if cli.json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "instance": config.name,
                "state": "booted",
                "qemu_pid": process.pid(),
                "adb_serial": adb_client.serial(),
                "qmp_port": config.qmp_port,
                "boot_seconds": boot_seconds.as_secs_f64(),
                "state_file": state_file,
                "log_file": config.log_file,
            }))?
        );
    } else if !cli.quiet {
        println!(
            "instance {}: QEMU pid {}, ADB {}, QMP {}",
            config.name,
            process.pid(),
            adb_client.serial(),
            config.qmp_port
        );
        println!(
            "instance {}: Android booted in {:.1} s",
            config.name,
            boot_seconds.as_secs_f64()
        );
        println!("Press Ctrl+C to power down.");
    }

    let interrupted = install_ctrlc_handler()?;
    let started = Instant::now();
    let reason = loop {
        if interrupted.load(Ordering::SeqCst) {
            break StopReason::Interrupted;
        }
        if let Some(status) = process.try_wait()? {
            break StopReason::Exited(status);
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    match reason {
        StopReason::Interrupted => {
            let mut qmp = match connect_qmp_retry(config.qmp_port, Duration::from_secs(5)) {
                Ok(client) => client,
                Err(error) => {
                    if process.try_wait()?.is_none() {
                        tracing::warn!(%error, "cannot reconnect to QMP; killing QEMU");
                        process.kill()?;
                        let _ = process.wait();
                    }
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string(&serde_json::json!({
                                "instance": config.name,
                                "state": "stopped",
                                "qemu_pid": process.pid(),
                                "shutdown_seconds": started.elapsed().as_secs_f64(),
                                "clean": false,
                            }))?
                        );
                    } else if !cli.quiet {
                        println!(
                            "instance {}: stopped without a clean QMP powerdown",
                            config.name
                        );
                    }
                    return Ok(1);
                }
            };
            shutdown_vm(&mut process, &mut qmp)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "instance": config.name,
                        "state": "stopped",
                        "qemu_pid": process.pid(),
                        "shutdown_seconds": started.elapsed().as_secs_f64(),
                        "clean": true,
                    }))?
                );
            } else if !cli.quiet {
                println!("instance {}: powered down cleanly", config.name);
            }
            Ok(0)
        }
        StopReason::Exited(status) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "instance": config.name,
                        "state": "exited",
                        "qemu_pid": process.pid(),
                        "exit_status": status.to_string(),
                    }))?
                );
            } else if !cli.quiet {
                println!("instance {}: QEMU exited with {status}", config.name);
            }
            Ok(if status.success() { 0 } else { 1 })
        }
    }
}

fn stop_instance(cli: &Cli, args: &StopArgs) -> Result<u8, CliError> {
    let data_dir = resolve_data_dir(cli)?;
    let run_directory = run_dir(&data_dir, &args.instance)?;
    if !state_path(&run_directory).is_file() {
        return Err(CliError::InstanceNotFound(args.instance.clone()));
    }
    let state = read_state(&run_directory)?;
    if !process_alive(state.qemu_pid) {
        let _ = remove_state(&run_directory);
        return Err(CliError::NotRunning(args.instance.clone()));
    }

    let mut qmp = connect_qmp_retry(state.qmp_port, Duration::from_secs(5))?;
    qmp.system_powerdown()?;
    let deadline = Instant::now() + Duration::from_secs(args.timeout);
    let mut stopped = false;
    while Instant::now() < deadline {
        if !process_alive(state.qemu_pid) {
            stopped = true;
            break;
        }
        match qmp.query_status() {
            Ok(status) if status.status == "shutdown" => {}
            Ok(_) => {}
            Err(QmpError::Closed) => {
                stopped = true;
                break;
            }
            Err(error) => return Err(error.into()),
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    if !stopped {
        let _ = qmp.quit();
        for _ in 0..50 {
            if !process_alive(state.qemu_pid) {
                stopped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    if !stopped {
        return Err(CliError::General(format!(
            "QEMU pid {} did not stop within {} s; see {}",
            state.qemu_pid,
            args.timeout,
            state.log_file.display()
        )));
    }
    let _ = remove_state(&run_directory);

    if cli.json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "instance": args.instance,
                "state": "stopped",
                "qemu_pid": state.qemu_pid,
            }))?
        );
    } else if !cli.quiet {
        println!(
            "instance {}: stopped (QEMU pid {})",
            args.instance, state.qemu_pid
        );
    }
    Ok(0)
}

fn resolve_data_dir(cli: &Cli) -> Result<PathBuf, CliError> {
    match &cli.data_dir {
        Some(directory) => Ok(directory.clone()),
        None => Ok(rsdroid_core::paths::data_dir()?),
    }
}

fn ensure_port_free(port: u16) -> Result<(), CliError> {
    std::net::TcpListener::bind(("127.0.0.1", port))
        .map(|_| ())
        .map_err(|source| CliError::Port { port, source })
}

fn connect_qmp_retry(port: u16, timeout: Duration) -> Result<QmpClient, QmpError> {
    let deadline = Instant::now() + timeout;
    loop {
        match QmpClient::connect_port(port, Duration::from_millis(250)) {
            Ok(client) => return Ok(client),
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(error);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn state_file_dir(config: &VmConfig) -> PathBuf {
    config
        .pid_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn abort_vm(process: &mut VmProcess, qmp: &mut QmpClient) {
    let _ = qmp.quit();
    let _ = process.wait_for_exit(Duration::from_secs(5));
    let _ = process.kill();
    let _ = process.wait();
}

fn shutdown_vm(process: &mut VmProcess, qmp: &mut QmpClient) -> Result<(), CliError> {
    if qmp.system_powerdown().is_err() {
        tracing::debug!("system_powerdown failed; trying quit");
    }
    if process.wait_for_exit(Duration::from_secs(30))?.is_none() {
        let _ = qmp.quit();
        if process.wait_for_exit(Duration::from_secs(5))?.is_none() {
            process.kill()?;
            let _ = process.wait();
        }
    }
    Ok(())
}

enum StopReason {
    Interrupted,
    Exited(std::process::ExitStatus),
}

fn install_ctrlc_handler() -> Result<Arc<AtomicBool>, CliError> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&interrupted);
    ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst))
        .map_err(|error| CliError::Signal(error.to_string()))?;
    Ok(interrupted)
}

struct StateCleanup {
    run_dir: PathBuf,
}

impl Drop for StateCleanup {
    fn drop(&mut self) {
        let _ = remove_state(&self.run_dir);
    }
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
