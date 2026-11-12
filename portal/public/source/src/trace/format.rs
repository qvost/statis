//! Version 1 trace encoding, implemented using only the standard library.
//!
//! The header is `STATIS\0\x01`, three 8-byte fields (seed, quantum, IEEE-754
//! fault rate), then a u32 argument count and u32-length-prefixed UTF-8 strings.
//! Events start with a one-byte tag (1: syscall, 2: RDTSC, 3: exit), followed by
//! their fields in declaration order, with no padding. All numbers and lengths
//! are little-endian. EOF is valid only between complete events.

use std::io::{self, Read, Write};

const MAGIC: &[u8; 8] = b"STATIS\0\x01";
const SYSCALL: u8 = 1;
const RDTSC: u8 = 2;
const THREAD_EXIT: u8 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct TraceHeader {
    pub seed: u64,
    pub quantum: u64,
    pub fault_rate: f64,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEvent {
    Syscall {
        tid: u32,
        nr: i32,
        args: [u64; 6],
        virtual_time_ns: u64,
        return_val: i64,
        error: i32,
    },
    Rdtsc {
        tid: u32,
        virtual_cycles: u64,
    },
    ThreadExit {
        tid: u32,
        exit_code: i32,
    },
}

pub struct TraceWriter<W: Write> {
    writer: W,
}

impl<W: Write> TraceWriter<W> {
    pub fn new(mut writer: W, header: TraceHeader) -> io::Result<Self> {
        let count = checked_length(header.command.len())?;
        // Reject unrepresentable lengths before writing any part of the header.
        for argument in &header.command {
            checked_length(argument.len())?;
        }
        writer.write_all(MAGIC)?;
        writer.write_all(&header.seed.to_le_bytes())?;
        writer.write_all(&header.quantum.to_le_bytes())?;
        writer.write_all(&header.fault_rate.to_le_bytes())?;
        writer.write_all(&count.to_le_bytes())?;
        for argument in &header.command {
            writer.write_all(&checked_length(argument.len())?.to_le_bytes())?;
            writer.write_all(argument.as_bytes())?;
        }
        Ok(Self { writer })
    }

    pub fn write_event(&mut self, event: &TraceEvent) -> io::Result<()> {
        match event {
            TraceEvent::Syscall {
                tid,
                nr,
                args,
                virtual_time_ns,
                return_val,
                error,
            } => {
                self.writer.write_all(&[SYSCALL])?;
                self.writer.write_all(&tid.to_le_bytes())?;
                self.writer.write_all(&nr.to_le_bytes())?;
                for argument in args {
                    self.writer.write_all(&argument.to_le_bytes())?;
                }
                self.writer.write_all(&virtual_time_ns.to_le_bytes())?;
                self.writer.write_all(&return_val.to_le_bytes())?;
                self.writer.write_all(&error.to_le_bytes())?;
            }
            TraceEvent::Rdtsc {
                tid,
                virtual_cycles,
            } => {
                self.writer.write_all(&[RDTSC])?;
                self.writer.write_all(&tid.to_le_bytes())?;
                self.writer.write_all(&virtual_cycles.to_le_bytes())?;
            }
            TraceEvent::ThreadExit { tid, exit_code } => {
                self.writer.write_all(&[THREAD_EXIT])?;
                self.writer.write_all(&tid.to_le_bytes())?;
                self.writer.write_all(&exit_code.to_le_bytes())?;
            }
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub struct TraceReader<R: Read> {
    reader: R,
    header: TraceHeader,
}

impl<R: Read> TraceReader<R> {
    pub fn new(mut reader: R) -> io::Result<Self> {
        if &read_array::<8>(&mut reader)? != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid statis trace magic or version",
            ));
        }
        let seed = u64::from_le_bytes(read_array(&mut reader)?);
        let quantum = u64::from_le_bytes(read_array(&mut reader)?);
        let fault_rate = f64::from_le_bytes(read_array(&mut reader)?);
        let count = u32::from_le_bytes(read_array(&mut reader)?);
        // Grow only as complete arguments arrive, never from an untrusted count.
        let mut command = Vec::new();
        for _ in 0..count {
            let argument = read_string(&mut reader)?;
            command
                .try_reserve(1)
                .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
            command.push(argument);
        }
        Ok(Self {
            reader,
            header: TraceHeader {
                seed,
                quantum,
                fault_rate,
                command,
            },
        })
    }

    pub fn header(&self) -> &TraceHeader {
        &self.header
    }

    /// Read one event, returning None only at a clean record boundary.
    pub fn next_event(&mut self) -> io::Result<Option<TraceEvent>> {
        let mut tag = [0_u8];
        loop {
            match self.reader.read(&mut tag) {
                Ok(0) => return Ok(None),
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        let event = match tag[0] {
            SYSCALL => {
                let tid = u32::from_le_bytes(read_array(&mut self.reader)?);
                let nr = i32::from_le_bytes(read_array(&mut self.reader)?);
                let mut args = [0; 6];
                for argument in &mut args {
                    *argument = u64::from_le_bytes(read_array(&mut self.reader)?);
                }
                TraceEvent::Syscall {
                    tid,
                    nr,
                    args,
                    virtual_time_ns: u64::from_le_bytes(read_array(&mut self.reader)?),
                    return_val: i64::from_le_bytes(read_array(&mut self.reader)?),
                    error: i32::from_le_bytes(read_array(&mut self.reader)?),
                }
            }
            RDTSC => TraceEvent::Rdtsc {
                tid: u32::from_le_bytes(read_array(&mut self.reader)?),
                virtual_cycles: u64::from_le_bytes(read_array(&mut self.reader)?),
            },
            THREAD_EXIT => TraceEvent::ThreadExit {
                tid: u32::from_le_bytes(read_array(&mut self.reader)?),
                exit_code: i32::from_le_bytes(read_array(&mut self.reader)?),
            },
            tag => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown trace event tag {tag}"),
                ));
            }
        };
        Ok(Some(event))
    }
}

fn checked_length(length: usize) -> io::Result<u32> {
    u32::try_from(length).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "trace command length exceeds u32",
        )
    })
}

fn read_array<const N: usize>(reader: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_string(reader: &mut impl Read) -> io::Result<String> {
    let length = u32::from_le_bytes(read_array(reader)?);
    let mut bytes = Vec::new();
    // Read incrementally: a forged multi-gigabyte prefix on a short input must
    // return UnexpectedEof without first allocating the advertised size.
    reader.take(u64::from(length)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != u64::from(length) {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "truncated trace command argument",
        ));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Dump events in recorded order. RDTSC cycles share the runtime's nanosecond
/// timeline; exits inherit the last event's timestamp, or zero for an empty past.
/// Unknown x86_64 syscall numbers are printed numerically as SYS_<nr>.
pub fn dump_trace<R: Read, W: Write>(reader: &mut TraceReader<R>, out: &mut W) -> io::Result<()> {
    let header = reader.header();
    writeln!(out, "statis trace v1")?;
    writeln!(out, "seed: {}", header.seed)?;
    writeln!(out, "quantum: {}", header.quantum)?;
    writeln!(out, "fault_rate: {}", header.fault_rate)?;
    writeln!(out, "command: {:?}", header.command)?;
    let mut now_ns = 0;
    while let Some(event) = reader.next_event()? {
        match event {
            TraceEvent::Syscall {
                tid,
                nr,
                args,
                virtual_time_ns,
                return_val,
                error,
            } => {
                now_ns = virtual_time_ns;
                write!(out, "[{now_ns} ns] tid={tid} SYS_")?;
                if let Some(name) = syscall_name(nr) {
                    write!(out, "{name}")?;
                } else {
                    write!(out, "{nr}")?;
                }
                write!(out, "(")?;
                for (index, argument) in args.iter().enumerate() {
                    if index != 0 {
                        write!(out, ", ")?;
                    }
                    write!(out, "{argument:#x}")?;
                }
                writeln!(out, ") -> {return_val} (error: {error})")?;
            }
            TraceEvent::Rdtsc {
                tid,
                virtual_cycles,
            } => {
                now_ns = virtual_cycles;
                writeln!(
                    out,
                    "[{now_ns} ns] tid={tid} RDTSC -> {virtual_cycles} cycles"
                )?;
            }
            TraceEvent::ThreadExit { tid, exit_code } => {
                writeln!(out, "[{now_ns} ns] tid={tid} EXIT -> code {exit_code}")?;
            }
        }
    }
    Ok(())
}

fn syscall_name(nr: i32) -> Option<&'static str> {
    Some(match nr {
        0 => "read",
        1 => "write",
        7 => "poll",
        24 => "sched_yield",
        35 => "nanosleep",
        39 => "getpid",
        42 => "connect",
        43 => "accept",
        44 => "sendto",
        45 => "recvfrom",
        46 => "sendmsg",
        47 => "recvmsg",
        56 => "clone",
        59 => "execve",
        60 => "exit",
        62 => "kill",
        96 => "gettimeofday",
        110 => "getppid",
        186 => "gettid",
        202 => "futex",
        228 => "clock_gettime",
        230 => "clock_nanosleep",
        231 => "exit_group",
        232 => "epoll_wait",
        234 => "tgkill",
        271 => "ppoll",
        281 => "epoll_pwait",
        288 => "accept4",
        318 => "getrandom",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> TraceHeader {
        TraceHeader {
            seed: 42,
            quantum: 10_000,
            fault_rate: 0.25,
            command: vec![
                "echo".into(),
                "hello world".into(),
                "λ\0雪".into(),
                String::new(),
            ],
        }
    }

    fn events() -> [TraceEvent; 3] {
        [
            TraceEvent::Syscall {
                tid: u32::MAX,
                nr: i32::MIN,
                args: [0, 1, u64::MAX, 0x1234_5678_9abc_def0, 4, 5],
                virtual_time_ns: u64::MAX,
                return_val: i64::MIN,
                error: -5,
            },
            TraceEvent::Rdtsc {
                tid: 7,
                virtual_cycles: u64::MAX,
            },
            TraceEvent::ThreadExit {
                tid: 7,
                exit_code: i32::MIN,
            },
        ]
    }

    fn encode(header: TraceHeader, events: &[TraceEvent]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut writer = TraceWriter::new(&mut bytes, header).unwrap();
        for event in events {
            writer.write_event(event).unwrap();
        }
        writer.flush().unwrap();
        bytes
    }

    #[test]
    fn roundtrip_header_and_all_event_types_including_integer_boundaries() {
        let expected = events();
        let bytes = encode(header(), &expected);
        let mut reader = TraceReader::new(bytes.as_slice()).unwrap();
        assert_eq!(reader.header(), &header());
        for event in expected {
            assert_eq!(reader.next_event().unwrap(), Some(event));
        }
        assert_eq!(reader.next_event().unwrap(), None);
        assert_eq!(reader.next_event().unwrap(), None);
    }

    #[test]
    fn empty_trace_and_float_bit_patterns_roundtrip() {
        for bits in [
            0,
            (-0.0_f64).to_bits(),
            f64::INFINITY.to_bits(),
            0x7ff8_0000_0000_0123,
        ] {
            let header = TraceHeader {
                fault_rate: f64::from_bits(bits),
                command: vec![],
                ..header()
            };
            let bytes = encode(header, &[]);
            let mut reader = TraceReader::new(bytes.as_slice()).unwrap();
            assert_eq!(reader.header().fault_rate.to_bits(), bits);
            assert!(reader.header().command.is_empty());
            assert_eq!(reader.next_event().unwrap(), None);
        }
    }

    #[test]
    fn binary_layout_is_little_endian_with_no_padding() {
        let golden_header = TraceHeader {
            seed: 0x0102_0304_0506_0708,
            quantum: 0x1112_1314_1516_1718,
            fault_rate: 0.5,
            command: vec![],
        };
        let events = [
            TraceEvent::Syscall {
                tid: 0x0102_0304,
                nr: -1,
                args: [1, 2, 3, 4, 5, 6],
                virtual_time_ns: 7,
                return_val: -2,
                error: -5,
            },
            TraceEvent::Rdtsc {
                tid: 1,
                virtual_cycles: 0x0102_0304_0506_0708,
            },
            TraceEvent::ThreadExit {
                tid: 2,
                exit_code: -3,
            },
        ];
        let bytes = encode(golden_header, &events);
        assert_eq!(bytes.len(), 36 + 77 + 13 + 9);
        assert_eq!(&bytes[..8], b"STATIS\0\x01");
        assert_eq!(&bytes[8..16], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(
            &bytes[16..24],
            &[0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11]
        );
        assert_eq!(&bytes[24..32], &[0, 0, 0, 0, 0, 0, 0xe0, 0x3f]);
        assert_eq!(&bytes[32..36], &[0, 0, 0, 0]);
        let syscall = &bytes[36..113];
        assert_eq!(syscall[0], 1);
        assert_eq!(&syscall[1..5], &[4, 3, 2, 1]);
        assert_eq!(&syscall[5..9], &[0xff; 4]);
        for (index, word) in syscall[9..57].as_chunks::<8>().0.iter().enumerate() {
            assert_eq!(word, &[index as u8 + 1, 0, 0, 0, 0, 0, 0, 0]);
        }
        assert_eq!(&syscall[57..65], &[7, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            &syscall[65..73],
            &[0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
        assert_eq!(&syscall[73..77], &[0xfb, 0xff, 0xff, 0xff]);
        assert_eq!(&bytes[113..126], &[2, 1, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(&bytes[126..], &[3, 2, 0, 0, 0, 0xfd, 0xff, 0xff, 0xff]);
        let command = encode(
            TraceHeader {
                command: vec!["é".into(), "".into()],
                ..header()
            },
            &[],
        );
        assert_eq!(
            &command[32..],
            &[2, 0, 0, 0, 2, 0, 0, 0, 0xc3, 0xa9, 0, 0, 0, 0]
        );
    }

    #[test]
    fn rejects_every_magic_mismatch_and_header_truncation() {
        let bytes = encode(header(), &[]);
        for index in 0..8 {
            let mut corrupt = bytes.clone();
            corrupt[index] ^= 1;
            let error = TraceReader::new(corrupt.as_slice()).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }
        for end in 0..bytes.len() {
            let error = TraceReader::new(&bytes[..end]).err().unwrap();
            assert_eq!(
                error.kind(),
                io::ErrorKind::UnexpectedEof,
                "header prefix length {end}"
            );
        }
    }

    #[test]
    fn rejects_every_partial_event_but_accepts_eof_between_events() {
        let empty_header = TraceHeader {
            command: vec![],
            ..header()
        };
        for event in events() {
            let bytes = encode(empty_header.clone(), &[event]);
            let mut reader = TraceReader::new(&bytes[..36]).unwrap();
            assert_eq!(reader.next_event().unwrap(), None);
            for end in 37..bytes.len() {
                let mut reader = TraceReader::new(&bytes[..end]).unwrap();
                let error = reader.next_event().unwrap_err();
                assert_eq!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof,
                    "event prefix length {}",
                    end - 36
                );
            }
        }
    }

    #[test]
    fn rejects_unknown_event_tags_and_invalid_utf8() {
        for tag in [0, 4, 255] {
            let mut bytes = encode(
                TraceHeader {
                    command: vec![],
                    ..header()
                },
                &[],
            );
            bytes.push(tag);
            let mut reader = TraceReader::new(bytes.as_slice()).unwrap();
            assert_eq!(
                reader.next_event().unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        let mut bytes = encode(
            TraceHeader {
                command: vec!["x".into()],
                ..header()
            },
            &[],
        );
        *bytes.last_mut().unwrap() = 0xff;
        assert_eq!(
            TraceReader::new(bytes.as_slice()).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn forged_lengths_do_not_require_eager_allocation() {
        let base = encode(
            TraceHeader {
                command: vec![],
                ..header()
            },
            &[],
        );
        let mut count = base.clone();
        count[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            TraceReader::new(count.as_slice()).err().unwrap().kind(),
            io::ErrorKind::UnexpectedEof
        );
        let mut length = base;
        length[32..36].copy_from_slice(&1_u32.to_le_bytes());
        length.extend(u32::MAX.to_le_bytes());
        length.extend(b"short");
        assert_eq!(
            TraceReader::new(length.as_slice()).err().unwrap().kind(),
            io::ErrorKind::UnexpectedEof
        );
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            checked_length(usize::MAX).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    struct Fragmented<'a> {
        bytes: &'a [u8],
        interrupt: bool,
    }

    impl Read for Fragmented<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            self.interrupt = !self.interrupt;
            if self.interrupt {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let count = out.len().min(self.bytes.len()).min(1);
            out[..count].copy_from_slice(&self.bytes[..count]);
            self.bytes = &self.bytes[count..];
            Ok(count)
        }
    }

    #[test]
    fn handles_fragmented_reads_and_interrupted_io() {
        let events = events();
        let bytes = encode(header(), &events);
        let mut reader = TraceReader::new(Fragmented {
            bytes: &bytes,
            interrupt: false,
        })
        .unwrap();
        assert_eq!(reader.header(), &header());
        for event in events {
            assert_eq!(reader.next_event().unwrap(), Some(event));
        }
        assert_eq!(reader.next_event().unwrap(), None);
    }

    struct BrokenReader;
    impl Read for BrokenReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }

    struct FlushFailure(Vec<u8>);
    impl Write for FlushFailure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn propagates_read_write_and_flush_failures() {
        assert_eq!(
            TraceReader::new(BrokenReader).err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        let bytes = encode(header(), &[]);
        let mut reader = TraceReader::new(bytes.as_slice().chain(BrokenReader)).unwrap();
        assert_eq!(
            reader.next_event().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        let mut short_header = [0_u8; 4];
        assert_eq!(
            TraceWriter::new(&mut short_header[..], header())
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::WriteZero
        );
        let mut short_event = vec![0; bytes.len() + 2];
        let mut writer = TraceWriter::new(short_event.as_mut_slice(), header()).unwrap();
        assert_eq!(
            writer.write_event(&events()[0]).unwrap_err().kind(),
            io::ErrorKind::WriteZero
        );
        let mut writer = TraceWriter::new(FlushFailure(vec![]), header()).unwrap();
        assert_eq!(
            writer.flush().unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn dump_prints_header_named_and_unknown_syscalls_and_event_times() {
        let header = TraceHeader {
            command: vec!["echo".into(), "hello world".into()],
            ..header()
        };
        let events = [
            TraceEvent::Syscall {
                tid: 7,
                nr: 228,
                args: [1, 0x2000, 0, 0, 0, 0],
                virtual_time_ns: 100,
                return_val: 0,
                error: 0,
            },
            TraceEvent::Rdtsc {
                tid: 7,
                virtual_cycles: 150,
            },
            TraceEvent::Syscall {
                tid: 9,
                nr: 999,
                args: [0; 6],
                virtual_time_ns: 200,
                return_val: -1,
                error: -38,
            },
            TraceEvent::ThreadExit {
                tid: 7,
                exit_code: 0,
            },
        ];
        let bytes = encode(header, &events);
        let mut reader = TraceReader::new(bytes.as_slice()).unwrap();
        let mut text = Vec::new();
        dump_trace(&mut reader, &mut text).unwrap();
        assert_eq!(
            String::from_utf8(text).unwrap(),
            concat!(
                "statis trace v1\nseed: 42\nquantum: 10000\nfault_rate: 0.25\ncommand: [\"echo\", \"hello world\"]\n",
                "[100 ns] tid=7 SYS_clock_gettime(0x1, 0x2000, 0x0, 0x0, 0x0, 0x0) -> 0 (error: 0)\n",
                "[150 ns] tid=7 RDTSC -> 150 cycles\n",
                "[200 ns] tid=9 SYS_999(0x0, 0x0, 0x0, 0x0, 0x0, 0x0) -> -1 (error: -38)\n",
                "[200 ns] tid=7 EXIT -> code 0\n",
            )
        );
    }

    #[test]
    fn dump_starts_exits_at_zero_and_propagates_errors() {
        let bytes = encode(
            header(),
            &[TraceEvent::ThreadExit {
                tid: 1,
                exit_code: -1,
            }],
        );
        let mut reader = TraceReader::new(bytes.as_slice()).unwrap();
        let mut text = Vec::new();
        dump_trace(&mut reader, &mut text).unwrap();
        assert!(
            String::from_utf8(text)
                .unwrap()
                .ends_with("[0 ns] tid=1 EXIT -> code -1\n")
        );
        let mut reader = TraceReader::new(bytes.as_slice()).unwrap();
        let mut full: &mut [u8] = &mut [];
        assert_eq!(
            dump_trace(&mut reader, &mut full).unwrap_err().kind(),
            io::ErrorKind::WriteZero
        );
        let mut truncated = bytes;
        truncated.pop();
        let mut reader = TraceReader::new(truncated.as_slice()).unwrap();
        assert_eq!(
            dump_trace(&mut reader, &mut io::sink()).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
