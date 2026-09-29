use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

const DEFAULT_CAPACITY: usize = 1000;
const BROADCAST_CAPACITY: usize = 256;

/// Thread-safe ring buffer that captures log lines.
#[derive(Clone)]
pub struct LogRingBuffer {
    inner: Arc<Mutex<RingBufferInner>>,
    tx: broadcast::Sender<String>,
}

struct RingBufferInner {
    lines: VecDeque<String>,
    capacity: usize,
    /// Accumulates partial writes (no trailing newline yet).
    ///
    /// Bytes, not a `String`: a write can split a multi-byte character, and the
    /// lossy conversion is done once per completed line rather than per
    /// fragment. The buffer is drained rather than replaced, so it keeps its
    /// capacity across lines instead of regrowing from zero every time.
    partial: Vec<u8>,
}

impl LogRingBuffer {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            inner: Arc::new(Mutex::new(RingBufferInner {
                lines: VecDeque::with_capacity(capacity),
                capacity,
                partial: Vec::new(),
            })),
            tx,
        }
    }

    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    pub fn snapshot(&self, limit: usize) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        let safe_limit = limit.min(inner.lines.len());
        inner
            .lines
            .iter()
            .skip(inner.lines.len().saturating_sub(safe_limit))
            .cloned()
            .collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }
}

impl Write for LogRingBuffer {
    /// Split the write into lines and push each completed one into the ring.
    ///
    /// `tracing_subscriber` calls this once per formatted event, so the common
    /// case is exactly one complete line. The old implementation walked the
    /// write a `char` at a time into a `String` that `mem::take` reset to zero
    /// capacity after every line, then cloned each line so the ring and the
    /// broadcast could each own one, and allocated a `Vec` per call whether or
    /// not a line completed — roughly ten allocations and ten memcpys per line.
    /// At the ~1,650 lines/s a warning flood produces that was a top allocation
    /// site in its own right.
    ///
    /// Semantics are unchanged: a line with no trailing newline stays partial
    /// across writes, empty lines are dropped, and the ring evicts from the
    /// front at capacity.
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Only materialize the broadcast copies somebody is listening for.
        let has_subscribers = self.tx.receiver_count() > 0;
        let mut new_lines: Vec<String> = Vec::new();
        let mut inner = self.inner.lock().unwrap();

        for segment in buf.split_inclusive(|byte| *byte == b'\n') {
            let Some(body) = segment.strip_suffix(b"\n") else {
                // No newline yet: this trails into the next write.
                inner.partial.extend_from_slice(segment);
                break;
            };
            // Convert once, for the whole line, and only when there is one.
            let line = if inner.partial.is_empty() {
                if body.is_empty() {
                    continue;
                }
                String::from_utf8_lossy(body).into_owned()
            } else {
                inner.partial.extend_from_slice(body);
                if inner.partial.is_empty() {
                    continue;
                }
                let line = String::from_utf8_lossy(&inner.partial).into_owned();
                // Drain, not `mem::take`: the buffer keeps its capacity.
                inner.partial.clear();
                line
            };
            if inner.lines.len() >= inner.capacity {
                inner.lines.pop_front();
            }
            if has_subscribers {
                new_lines.push(line.clone());
            }
            inner.lines.push_back(line);
        }
        drop(inner);
        for line in new_lines {
            let _ = self.tx.send(line);
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Adapter that lets `tracing_subscriber` write to our ring buffer.
/// Implements `tracing_subscriber::fmt::MakeWriter` by returning a clone
/// of the buffer (which implements `io::Write`).
#[derive(Clone)]
pub(crate) struct LogBufferWriter {
    buffer: LogRingBuffer,
}

impl LogBufferWriter {
    pub fn new(buffer: LogRingBuffer) -> Self {
        Self { buffer }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBufferWriter {
    type Writer = LogRingBuffer;

    fn make_writer(&'a self) -> Self::Writer {
        self.buffer.clone()
    }
}

#[path = "log_file.rs"]
mod file;
pub(crate) use file::{LogFilePolicy, open_log_file};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_buffer_joins_a_line_split_across_writes() {
        let mut buffer = LogRingBuffer::new(8);
        buffer.write_all(b"first ").expect("write");
        buffer.write_all(b"second ").expect("write");
        assert!(
            buffer.snapshot(8).is_empty(),
            "a line is only complete at its newline"
        );
        buffer.write_all(b"third\n").expect("write");
        assert_eq!(buffer.snapshot(8), vec!["first second third".to_string()]);
    }

    #[test]
    fn ring_buffer_splits_several_lines_in_one_write() {
        let mut buffer = LogRingBuffer::new(8);
        buffer
            .write_all(b"one\ntwo\n\nthree\ntrailing")
            .expect("write");
        assert_eq!(
            buffer.snapshot(8),
            vec!["one".to_string(), "two".to_string(), "three".to_string()],
            "empty lines are dropped and the trailing fragment stays partial"
        );
        buffer.write_all(b"\n").expect("write");
        assert_eq!(
            buffer.snapshot(8).last().map(String::as_str),
            Some("trailing")
        );
    }

    #[test]
    fn ring_buffer_evicts_the_oldest_line_at_capacity() {
        let mut buffer = LogRingBuffer::new(2);
        buffer.write_all(b"a\nb\nc\n").expect("write");
        assert_eq!(buffer.snapshot(8), vec!["b".to_string(), "c".to_string()]);
    }

    #[tokio::test]
    async fn ring_buffer_broadcasts_completed_lines_to_subscribers() {
        let mut buffer = LogRingBuffer::new(8);
        let mut rx = buffer.subscribe();
        buffer.write_all(b"hello\nworld\n").expect("write");
        assert_eq!(rx.recv().await.expect("first line"), "hello");
        assert_eq!(rx.recv().await.expect("second line"), "world");
    }

    #[test]
    fn ring_buffer_replaces_invalid_utf8_without_panicking() {
        let mut buffer = LogRingBuffer::new(8);
        // A lone continuation byte, and a two-byte sequence torn across writes.
        buffer.write_all(b"bad \xff byte\n").expect("write");
        buffer.write_all(b"split \xc3").expect("write");
        buffer.write_all(b"\xa9 ok\n").expect("write");
        let lines = buffer.snapshot(8);
        assert_eq!(lines[0], "bad \u{fffd} byte");
        assert_eq!(
            lines[1], "split é ok",
            "a character torn across writes is rejoined before decoding"
        );
    }
}
