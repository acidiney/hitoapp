use axum::body::{Body, Bytes};
use futures_util::Stream;
use std::{
    future::Future,
    io::{self, Read, Seek, SeekFrom},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Instant,
};
use tokio::{sync::OwnedSemaphorePermit, task::JoinHandle};

pub(crate) fn body<R: Read + Seek + Send + Unpin + 'static>(
    input: R,
    first: u64,
    length: u64,
    permit: OwnedSemaphorePermit,
) -> Body {
    let cancelled = Arc::new(AtomicBool::new(false));
    let delivery = Delivery {
        _permit: permit,
        expected: length,
        delivered: 0,
        failed: false,
        started: Instant::now(),
        span: tracing::Span::current(),
        dispatch: tracing::dispatcher::get_default(Clone::clone),
    };
    let worker = Worker {
        input,
        offset: Some(first),
        remaining: length,
        cancelled: cancelled.clone(),
        delivery,
    };
    Body::from_stream(AudioStream {
        state: Some(State::Ready(worker)),
        cancelled,
    })
}
struct Worker<R> {
    input: R,
    offset: Option<u64>,
    remaining: u64,
    cancelled: Arc<AtomicBool>,
    delivery: Delivery,
}
impl<R: Read + Seek> Worker<R> {
    fn read_chunk(&mut self) -> io::Result<Bytes> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if let Some(offset) = self.offset.take() {
            self.input.seek(SeekFrom::Start(offset))?;
        }
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let mut bytes = vec![0; self.remaining.min(65536) as usize];
        let count = self.input.read(&mut bytes)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Audio ended before its declared length",
            ));
        }
        self.remaining -= count as u64;
        bytes.truncate(count);
        Ok(Bytes::from(bytes))
    }
}
enum State<R> {
    Ready(Worker<R>),
    Reading(JoinHandle<(Worker<R>, io::Result<Bytes>)>),
}
struct AudioStream<R> {
    state: Option<State<R>>,
    cancelled: Arc<AtomicBool>,
}
impl<R: Read + Seek + Send + Unpin + 'static> Stream for AudioStream<R> {
    type Item = io::Result<Bytes>;
    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            match this.state.take() {
                None => return Poll::Ready(None),
                Some(State::Ready(mut worker)) => {
                    if worker.remaining == 0 {
                        return Poll::Ready(None);
                    }
                    this.state = Some(State::Reading(tokio::task::spawn_blocking(move || {
                        let chunk = worker.read_chunk();
                        (worker, chunk)
                    })));
                }
                Some(State::Reading(mut job)) => match Pin::new(&mut job).poll(context) {
                    Poll::Pending => {
                        this.state = Some(State::Reading(job));
                        return Poll::Pending;
                    }
                    Poll::Ready(Ok((mut worker, chunk))) => {
                        worker.delivery.observe(&chunk);
                        if chunk.is_ok() {
                            this.state = Some(State::Ready(worker));
                        }
                        return Poll::Ready(Some(chunk));
                    }
                    Poll::Ready(Err(_)) => {
                        return Poll::Ready(Some(Err(io::Error::other(
                            "Audio read worker failed",
                        ))));
                    }
                },
            }
        }
    }
}
impl<R> Drop for AudioStream<R> {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
struct Delivery {
    _permit: OwnedSemaphorePermit,
    expected: u64,
    delivered: u64,
    failed: bool,
    started: Instant,
    span: tracing::Span,
    dispatch: tracing::Dispatch,
}
impl Delivery {
    fn observe(&mut self, chunk: &io::Result<Bytes>) {
        match chunk {
            Ok(bytes) => self.delivered += bytes.len() as u64,
            Err(_) => self.failed = true,
        }
    }
}
impl Drop for Delivery {
    fn drop(&mut self) {
        tracing::dispatcher::with_default(&self.dispatch, || {
            let _entered = self.span.enter();
            let outcome = if self.failed {
                "read_failed"
            } else if self.expected == self.delivered {
                "complete"
            } else {
                "closed"
            };
            tracing::debug!(
                event = "folder_audio_finished",
                outcome,
                bytes = self.delivered,
                elapsed_ms = self.started.elapsed().as_millis() as u64
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;
    use http_body_util::BodyExt;
    use std::{
        fs::File,
        io::Write,
        sync::{Condvar, Mutex, atomic::AtomicUsize},
    };
    use tokio::sync::{Semaphore, mpsc};
    struct Gate {
        open: Mutex<bool>,
        ready: Condvar,
        entered: mpsc::UnboundedSender<()>,
        closed: AtomicUsize,
    }
    struct Release(Arc<Gate>);
    impl Drop for Release {
        fn drop(&mut self) {
            *self.0.open.lock().unwrap() = true;
            self.0.ready.notify_all();
        }
    }
    struct GatedFile {
        file: Option<File>,
        gate: Arc<Gate>,
    }
    impl Read for GatedFile {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.gate.entered.send(()).unwrap();
            let mut open = self.gate.open.lock().unwrap();
            while !*open {
                open = self.gate.ready.wait(open).unwrap();
            }
            drop(open);
            self.file.as_mut().unwrap().read(bytes)
        }
    }
    impl Seek for GatedFile {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.file.as_mut().unwrap().seek(position)
        }
    }
    impl Drop for GatedFile {
        fn drop(&mut self) {
            drop(self.file.take());
            self.gate.closed.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[tokio::test]
    async fn cancelled_pending_reads_keep_capacity_until_the_worker_closes_the_file() {
        let slots = Arc::new(Semaphore::new(8));
        let (entered, mut pending) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate {
            open: Mutex::new(false),
            ready: Condvar::new(),
            entered,
            closed: AtomicUsize::new(0),
        });
        let release = Release(gate.clone());
        for _ in 0..8 {
            let mut file = tempfile::tempfile().unwrap();
            file.write_all(b"owned audio fixture").unwrap();
            let reader = GatedFile {
                file: Some(file),
                gate: gate.clone(),
            };
            let mut audio = body(reader, 0, 19, slots.clone().try_acquire_owned().unwrap());
            assert!(audio.frame().now_or_never().is_none());
            tokio::time::timeout(std::time::Duration::from_secs(2), pending.recv())
                .await
                .unwrap()
                .unwrap();
            drop(audio);
        }
        assert_eq!(gate.closed.load(Ordering::Relaxed), 0);
        assert!(
            slots.clone().try_acquire_owned().is_err(),
            "cancelled body released admission while its read still owns the file"
        );
        drop(release);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while gate.closed.load(Ordering::Relaxed) != 8 || slots.available_permits() != 8 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
