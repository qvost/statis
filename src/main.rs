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
        #[arg(required = true, num_args = 1.., last = true)]
        command: Vec<String>,
    },
}

fn run() -> Result<i32> {
    match Cli::parse().command {
        Command::Run {
            seed,
            quantum,
            command,
        } => {
            // The launcher accepts argv including argv[0]; preserve all target arguments.
            let (child, listener) =
                statis::launcher::launch_child(Path::new(&command[0]), &command)?;
            statis::supervisor::run_supervisor(child, listener, seed, quantum)
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
