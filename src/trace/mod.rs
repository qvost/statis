//! Portable binary execution traces and their text representation.

pub mod format;

pub use format::{TraceEvent, TraceHeader, TraceReader, TraceWriter, dump_trace};
