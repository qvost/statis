//! Portable binary execution traces and their text representation.

pub mod format;

pub use format::{TraceEvent, TraceHeader, TraceReader, TraceWriter, dump_trace};

use std::io::{BufReader, BufWriter, Read, Write};

pub const TRACE_BUFFER_CAPACITY: usize = 65_536;

/// Use the same streaming buffer size for record, replay, and trace dumping.
pub fn buffered_reader<R: Read>(reader: R) -> BufReader<R> {
    BufReader::with_capacity(TRACE_BUFFER_CAPACITY, reader)
}

pub fn buffered_writer<W: Write>(writer: W) -> BufWriter<W> {
    BufWriter::with_capacity(TRACE_BUFFER_CAPACITY, writer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    struct CountWrites {
        bytes: Vec<u8>,
        calls: usize,
    }

    impl Write for CountWrites {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct CountReads<'a> {
        bytes: &'a [u8],
        calls: &'a Cell<usize>,
    }

    impl Read for CountReads<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.calls.set(self.calls.get() + 1);
            self.bytes.read(bytes)
        }
    }

    #[test]
    fn trace_streaming_batches_a_thousand_records_into_two_writes() {
        let mut out = buffered_writer(CountWrites::default());
        assert_eq!(out.capacity(), 65_536);
        let event = TraceEvent::Syscall {
            tid: 1,
            nr: 228,
            args: [0; 6],
            virtual_time_ns: 100,
            return_val: 0,
            error: 0,
        };
        let mut writer = TraceWriter::new(
            &mut out,
            TraceHeader {
                seed: 42,
                quantum: 10_000,
                fault_rate: 0.0,
                command: vec![],
            },
        )
        .unwrap();
        for _ in 0..1000 {
            writer.write_event(&event).unwrap();
        }
        writer.flush().unwrap();
        assert_eq!(out.get_ref().calls, 2);
        assert_eq!(out.get_ref().bytes.len(), 36 + 1000 * 77);
        let calls = Cell::new(0);
        let input = buffered_reader(CountReads {
            bytes: &out.get_ref().bytes,
            calls: &calls,
        });
        assert_eq!(input.capacity(), 65_536);
        let mut reader = TraceReader::new(input).unwrap();
        for _ in 0..1000 {
            assert_eq!(reader.next_event().unwrap(), Some(event.clone()));
        }
        assert_eq!(reader.next_event().unwrap(), None);
        assert_eq!(calls.get(), 3); // two data reads and one EOF check
    }
}
