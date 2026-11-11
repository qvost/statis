//! Execution statistics for the supervisor.

use std::{
    collections::BTreeMap,
    io::{self, Write},
    time::{Duration, Instant},
};

#[derive(Default, Debug)]
pub struct ExecutionTelemetry {
    pub real_start: Option<Instant>,
    pub real_duration: Option<Duration>,
    pub virtual_start_ns: u64,
    pub virtual_end_ns: u64,
    pub syscall_counts: BTreeMap<&'static str, u64>,
    pub context_switches: u64,
    pub rdtsc_traps: u64,
    pub faults_injected: u64,
    pub fault_counts: BTreeMap<&'static str, u64>,
    pub exit_code: Option<i32>,
}

impl ExecutionTelemetry {
    pub fn record_syscall(&mut self, name: &'static str) {
        *self.syscall_counts.entry(name).or_default() += 1;
    }

    pub fn record_fault(&mut self, syscall_name: &'static str) {
        self.faults_injected += 1;
        *self.fault_counts.entry(syscall_name).or_default() += 1;
    }

    fn fault_summary(&self) -> String {
        use std::fmt::Write;
        let mut output = String::new();
        if self.faults_injected > 0 {
            writeln!(output, "\nFaults").unwrap();
            writeln!(
                output,
                "  {:<20} {:>12}",
                "Injected",
                grouped(self.faults_injected)
            )
            .unwrap();
            for (name, count) in &self.fault_counts {
                writeln!(output, "  {name:<20} {:>12}", grouped(*count)).unwrap();
            }
        }
        output
    }

    pub fn finish(&mut self, virtual_end_ns: u64) {
        self.virtual_end_ns = virtual_end_ns;
        if self.real_duration.is_none() {
            self.real_duration = self.real_start.map(|start| start.elapsed());
        }
    }

    pub fn print_summary(&self) {
        // Summary output is best effort; a closed stderr must not change the
        // target's exit status. The fallible writer API is available to callers.
        let stderr = io::stderr();
        let _ = self.write_summary(&mut stderr.lock());
    }

    pub fn write_summary(&self, out: &mut impl Write) -> io::Result<()> {
        let virtual_seconds =
            self.virtual_end_ns.saturating_sub(self.virtual_start_ns) as f64 / 1e9;
        writeln!(out, "statis execution")?;
        writeln!(out, "  {:<20} {virtual_seconds:>12.3} s", "Virtual time")?;
        match self.real_duration {
            Some(duration) => {
                writeln!(
                    out,
                    "  {:<20} {:>12.3} s",
                    "Elapsed",
                    duration.as_secs_f64()
                )?;
                if !duration.is_zero() {
                    writeln!(
                        out,
                        "  {:<20} {:>12.1}x",
                        "Virtual / elapsed",
                        virtual_seconds / duration.as_secs_f64()
                    )?;
                }
            }
            None => writeln!(out, "  {:<20} {:>12}", "Elapsed", "unavailable")?,
        }
        writeln!(out, "\nSyscalls")?;
        if self.syscall_counts.is_empty() {
            writeln!(out, "  No trapped syscalls")?;
        }
        for (name, count) in &self.syscall_counts {
            writeln!(out, "  {name:<20} {:>12}", grouped(*count))?;
        }
        write!(out, "{}", self.fault_summary())?;
        writeln!(out, "\nRuntime")?;
        writeln!(
            out,
            "  {:<20} {:>12}",
            "RDTSC traps",
            grouped(self.rdtsc_traps)
        )?;
        writeln!(
            out,
            "  {:<20} {:>12}",
            "Context switches",
            grouped(self.context_switches)
        )?;
        if let Some(code) = self.exit_code {
            writeln!(out, "  {:<20} {code:>12}", "Exit code")?;
        }
        Ok(())
    }
}

fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i != 0 && (digits.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fault_summary_groups_and_omits_empty() {
        let mut telemetry = ExecutionTelemetry::default();
        assert!(telemetry.fault_summary().is_empty());
        for _ in 0..1_234 {
            telemetry.record_fault("read");
        }
        telemetry.record_fault("write");
        assert_eq!(telemetry.faults_injected, 1_235);
        assert_eq!(
            telemetry.fault_summary(),
            concat!(
                "\nFaults\n",
                "  Injected                    1,235\n",
                "  read                        1,234\n",
                "  write                           1\n"
            )
        );
    }
    #[test]
    fn records_and_finishes() {
        let mut telemetry = ExecutionTelemetry {
            real_start: Some(Instant::now()),
            virtual_start_ns: 10,
            ..Default::default()
        };
        telemetry.record_syscall("futex");
        telemetry.record_syscall("futex");
        telemetry.finish(100);
        assert_eq!(telemetry.syscall_counts["futex"], 2);
        assert_eq!(telemetry.virtual_end_ns, 100);
        assert!(telemetry.real_duration.is_some());
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
