use anyhow::Result;
use clap::{Parser, Subcommand};
use statis::{
    interceptor::{FaultConfig, FaultErrno},
    supervisor::{SupervisorConfig, TraceMode},
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process,
};

#[derive(Parser)]
#[command(version, about = "Deterministic userspace Linux runtime")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a command with deterministic syscall emulation.
    Run {
        #[arg(long, default_value_t = 0xDEADBEEF)]
        seed: u64,
        #[arg(long, default_value_t = 10_000)]
        quantum: u64,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
        summary: bool,
        #[arg(long, default_value_t = 0.0)]
        fault_rate: f64,
        #[arg(long, default_value = "econnreset,etimeout,eagain,eintr,eio,epipe")]
        fault_types: String,
        #[arg(required = true, num_args = 1.., last = true)]
        command: Vec<String>,
    },
    /// Record an execution trace.
    Record {
        #[arg(long, default_value = "statis.trace")]
        trace: PathBuf,
        #[arg(long, default_value_t = 0xDEADBEEF)]
        seed: u64,
        #[arg(long, default_value_t = 10_000)]
        quantum: u64,
        #[arg(long, default_value_t = 0.0)]
        fault_rate: f64,
        #[arg(long, default_value = "econnreset,etimeout,eagain,eintr,eio,epipe")]
        fault_types: String,
        #[arg(required = true, num_args = 1.., last = true)]
        command: Vec<String>,
    },
    /// Verify execution against a trace.
    Replay {
        #[arg(long, default_value = "statis.trace")]
        trace: PathBuf,
        #[arg(required = true, num_args = 1.., last = true)]
        command: Vec<String>,
    },
    /// Print trace events.
    TraceDump {
        #[arg(required = true)]
        trace: PathBuf,
    },
}

fn fault_config(rate: f64, types: &str) -> Result<FaultConfig> {
    anyhow::ensure!(
        rate.is_finite() && (0.0..=1.0).contains(&rate),
        "fault rate must be between 0 and 1"
    );
    let fault_types = types
        .split(',')
        .map(|name| {
            FaultErrno::from_name(name.trim())
                .ok_or_else(|| anyhow::anyhow!("unknown fault type: {name}"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(FaultConfig {
        fault_rate: rate,
        fault_types,
    })
}

fn execute(command: Vec<String>, mut config: SupervisorConfig, summary: bool) -> Result<i32> {
    let (child, listener) = statis::launcher::launch_child_with_options(
        Path::new(&command[0]),
        &command,
        &statis::launcher::LaunchOptions {
            intercept_io: config.fault_config.is_enabled(),
        },
    )?;
    config.command = command;
    let mut telemetry = statis::telemetry::ExecutionTelemetry::default();
    let result =
        statis::supervisor::run_supervisor_with_config(child, listener, config, &mut telemetry);
    if summary {
        telemetry.print_summary();
    }
    result
}

fn run() -> Result<i32> {
    match Cli::parse().command {
        Command::Run {
            seed,
            quantum,
            summary,
            fault_rate,
            fault_types,
            command,
        } => execute(
            command,
            SupervisorConfig {
                seed,
                quantum,
                fault_config: fault_config(fault_rate, &fault_types)?,
                trace_mode: TraceMode::None,
                command: Vec::new(),
            },
            summary,
        ),
        Command::Record {
            trace,
            seed,
            quantum,
            fault_rate,
            fault_types,
            command,
        } => execute(
            command,
            SupervisorConfig {
                seed,
                quantum,
                fault_config: fault_config(fault_rate, &fault_types)?,
                trace_mode: TraceMode::Record(trace),
                command: Vec::new(),
            },
            true,
        ),
        Command::Replay { trace, command } => {
            let reader = statis::trace::TraceReader::new(statis::trace::buffered_reader(
                std::fs::File::open(&trace)?,
            ))?;
            let recorded = reader.header();
            execute(
                command,
                SupervisorConfig {
                    seed: recorded.seed,
                    quantum: recorded.quantum,
                    fault_config: FaultConfig {
                        fault_rate: recorded.fault_rate,
                        ..Default::default()
                    },
                    trace_mode: TraceMode::Replay(trace),
                    command: Vec::new(),
                },
                true,
            )
        }
        Command::TraceDump { trace } => {
            let file = std::fs::File::open(trace)?;
            let mut reader = statis::trace::TraceReader::new(statis::trace::buffered_reader(file))?;
            let stdout = std::io::stdout();
            let mut out = statis::trace::buffered_writer(stdout.lock());
            statis::trace::dump_trace(&mut reader, &mut out)?;
            out.flush()?;
            Ok(0)
        }
    }
}

fn main() {
    env_logger::init();
    match run() {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("statis: {error:#}");
            process::exit(1);
        }
    }
}
