use std::io::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

const QUEUED_EVENTS: usize = 128;
const MAX_EVENT_BYTES: usize = 16 * 1024;
const SHUTDOWN_BOUND: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct LogDelivery {
    sender: mpsc::SyncSender<Vec<u8>>,
    sink: &'static str,
    stopping: Arc<AtomicBool>,
}

pub(crate) struct DeliveryGuard {
    delivery: LogDelivery,
    stopping: Arc<AtomicBool>,
    finished: mpsc::Receiver<()>,
}

impl LogDelivery {
    pub(crate) fn new(
        sink: &'static str,
        mut write: impl FnMut(&[u8]) -> io::Result<()> + Send + 'static,
    ) -> io::Result<(Self, DeliveryGuard)> {
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(QUEUED_EVENTS);
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let (finished_tx, finished) = mpsc::channel();
        std::thread::Builder::new()
            .name(format!("log-{sink}"))
            .spawn(move || {
                loop {
                    let event = if stop.load(Ordering::Acquire) {
                        receiver.try_recv().ok()
                    } else {
                        receiver.recv().ok()
                    };
                    let Some(event) = event else { break };
                    if !event.is_empty() && write(&event).is_err() {
                        metrics::counter!("scryer_log_write_errors_total", "sink" => sink)
                            .increment(1);
                    }
                }
                // Release sink resources on this thread, including file-log shutdown.
                drop(write);
                let _ = finished_tx.send(());
            })?;
        let delivery = Self {
            sender,
            sink,
            stopping: stopping.clone(),
        };
        let guard = DeliveryGuard {
            delivery: delivery.clone(),
            stopping,
            finished,
        };
        Ok((delivery, guard))
    }
}

impl Drop for DeliveryGuard {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        // Wake an idle worker. A full queue already supplies its wake-up.
        let _ = self.delivery.sender.try_send(Vec::new());
        // A failed disk or blocked stdout must not prevent process shutdown.
        let _ = self.finished.recv_timeout(SHUTDOWN_BOUND);
    }
}

pub(crate) struct EventWriter<'a> {
    delivery: &'a LogDelivery,
    bytes: Vec<u8>,
    oversized: bool,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogDelivery {
    type Writer = EventWriter<'a>;
    fn make_writer(&'a self) -> Self::Writer {
        EventWriter {
            delivery: self,
            bytes: Vec::new(),
            oversized: false,
        }
    }
}

impl Write for EventWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > MAX_EVENT_BYTES {
            self.oversized = true;
            self.bytes.clear();
        } else if !self.oversized {
            self.bytes.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for EventWriter<'_> {
    fn drop(&mut self) {
        // Enqueue complete events: never interleave fragments or emit truncated JSON.
        if self.oversized
            || self.delivery.stopping.load(Ordering::Acquire)
            || self
                .delivery
                .sender
                .try_send(std::mem::take(&mut self.bytes))
                .is_err()
        {
            metrics::counter!("scryer_log_dropped_events_total", "sink" => self.delivery.sink)
                .increment(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::fmt::MakeWriter;

    #[test]
    fn blocked_sink_and_full_queue_leave_runtime_responsive() {
        let (entered_tx, entered) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (written_tx, written) = mpsc::channel();
        let mut first = true;
        let (writer, guard) = LogDelivery::new("fixture", move |_| {
            if first {
                first = false;
                entered_tx.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(30)).unwrap();
            }
            written_tx.send(()).unwrap();
            Ok(())
        })
        .unwrap();
        writer.make_writer().write_all(b"blocked").unwrap();
        entered.recv_timeout(Duration::from_secs(30)).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .unwrap();
        let (done_tx, done) = mpsc::channel();
        for _ in 0..2 {
            let writer = writer.clone();
            let done_tx = done_tx.clone();
            runtime.spawn(async move {
                for _ in 0..QUEUED_EVENTS * 2 {
                    writer.make_writer().write_all(b"queued").unwrap();
                }
                done_tx.send(()).unwrap();
            });
        }
        let responsive = (0..2).all(|_| done.recv_timeout(Duration::from_secs(30)).is_ok());
        release.send(()).unwrap();
        drop(guard);
        runtime.shutdown_background();
        assert!(responsive);
        for _ in 0..=QUEUED_EVENTS {
            written.recv_timeout(Duration::from_secs(30)).unwrap();
        }
        assert!(matches!(
            written.recv_timeout(Duration::from_secs(30)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn complete_events_drain_in_order_and_oversized_events_are_omitted() {
        let (tx, rx) = mpsc::channel();
        let (writer, guard) = LogDelivery::new("fixture", move |bytes| {
            tx.send(bytes.to_vec()).unwrap();
            Ok(())
        })
        .unwrap();
        {
            let mut event = writer.make_writer();
            event.write_all(b"one").unwrap();
            event.write_all(b"\n").unwrap();
        }
        writer
            .make_writer()
            .write_all(&vec![0; MAX_EVENT_BYTES + 1])
            .unwrap();
        writer.make_writer().write_all(b"two\n").unwrap();
        drop(guard);
        assert_eq!(rx.recv_timeout(Duration::from_secs(30)).unwrap(), b"one\n");
        assert_eq!(rx.recv_timeout(Duration::from_secs(30)).unwrap(), b"two\n");
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(30)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn sink_error_does_not_stop_later_events() {
        let (tx, rx) = mpsc::channel();
        let (writer, guard) = LogDelivery::new("fixture", move |bytes| {
            if bytes == b"fail" {
                return Err(io::Error::other("fixture failure"));
            }
            tx.send(bytes.to_vec()).unwrap();
            Ok(())
        })
        .unwrap();
        writer.make_writer().write_all(b"fail").unwrap();
        writer.make_writer().write_all(b"recover").unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(30)).unwrap(),
            b"recover"
        );
        drop(guard);
    }
}
