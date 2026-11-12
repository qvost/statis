use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use statis::{interceptor::{FaultConfig,FaultErrno},trace::{TraceWriter,TraceHeader,TraceEvent}};
fn main() {
 use std::fmt::Write;
 let mut json=String::new();
 writeln!(json,"{{\"generator\":\"rand_chacha 0.3.1 / rand_core 0.6.4\",\"seeds\":[").unwrap();
 for (index, seed) in [0_u64,42,0xDEADBEEF,u64::MAX].iter().enumerate() {
  if index>0 { writeln!(json,",").unwrap(); }
  let mut rng=ChaCha20Rng::seed_from_u64(*seed);
  let values:Vec<String>=(0..40).map(|_|format!("\"{}\"",rng.next_u64())).collect();
  let config=FaultConfig{fault_rate:0.3,fault_types:vec![FaultErrno::Econnreset,FaultErrno::Etimedout,FaultErrno::Eagain]};
  let mut rng=ChaCha20Rng::seed_from_u64(*seed);
  let decisions:Vec<String>=(0..128).map(|_|format!("{}",config.should_fault(0,&[3,0,32,0,0,0],&mut rng).unwrap_or(0))).collect();
  write!(json,"{{\"seed\":\"{seed}\",\"u64\":[{}],\"faults\":[{}]}}",values.join(","),decisions.join(",")).unwrap();
 }
 writeln!(json,"]}}").unwrap();
 std::fs::write(std::env::args().nth(2).unwrap(),json).unwrap();
 let trace_path=std::env::args().nth(1).unwrap();
 let file=std::fs::File::create(trace_path).unwrap();
 let mut w=TraceWriter::new(file,TraceHeader{seed:42,quantum:10000,fault_rate:0.0,command:vec!["test_record_replay".into()]}).unwrap();
 for event in [TraceEvent::Syscall{tid:1000,nr:228,args:[1,0x1000,0,0,0,0],virtual_time_ns:100,return_val:0,error:0},TraceEvent::Rdtsc{tid:1000,virtual_cycles:150},TraceEvent::ThreadExit{tid:1000,exit_code:0}] {w.write_event(&event).unwrap();}w.flush().unwrap();
}
