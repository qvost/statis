use std::{path::Path, process};

use anyhow::Result;
use clap::{Parser, Subcommand};

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
        } => {
            anyhow::ensure!(
                fault_rate.is_finite() && (0.0..=1.0).contains(&fault_rate),
                "fault rate must be between 0 and 1"
            );
            let fault_types = fault_types
                .split(',')
                .map(|name| {
                    statis::interceptor::FaultErrno::from_name(name.trim())
                        .ok_or_else(|| anyhow::anyhow!("unknown fault type: {name}"))
                })
                .collect::<Result<Vec<_>>>()?;
            let fault_config = statis::interceptor::FaultConfig {
                fault_rate,
                fault_types,
            };
            // The launcher accepts argv including argv[0]; preserve all target arguments.
            let (child, listener) = statis::launcher::launch_child_with_options(
                Path::new(&command[0]),
                &command,
                &statis::launcher::LaunchOptions {
                    intercept_io: fault_config.is_enabled(),
                },
            )?;
            let mut telemetry = statis::telemetry::ExecutionTelemetry::default();
            let result = statis::supervisor::run_supervisor_with_config(
                child,
                listener,
                statis::supervisor::SupervisorConfig {
                    seed,
                    quantum,
                    fault_config,
                },
                &mut telemetry,
            );
            if summary {
                telemetry.print_summary();
            }
            result
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
