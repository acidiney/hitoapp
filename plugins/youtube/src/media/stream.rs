use axum::body::Bytes;
use futures_util::Stream;
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::{sync::CancellationToken, task::task_tracker::TaskTrackerToken};

const MAX_OUTPUT_BYTES: usize = 65_536;

pub(super) struct Connection {
    stream: Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send>>,
    pending: Bytes,
    remaining: u64,
    _permit: OwnedSemaphorePermit,
    _token: TaskTrackerToken,
    started: Instant,
    outcome: &'static str,
    span: tracing::Span,
    dispatch: tracing::Dispatch,
}

pub(super) type Shared = Arc<Mutex<Option<Connection>>>;

pub(super) struct Audio {
    connection: Shared,
    canceled: Pin<Box<dyn Future<Output = ()> + Send>>,
    idle: Option<Pin<Box<tokio::time::Sleep>>>,
    finished: bool,
}

impl Audio {
    pub fn new(
        response: super::transport::Body,
        length: u64,
        permit: OwnedSemaphorePermit,
        token: TaskTrackerToken,
        stopped: CancellationToken,
    ) -> (Self, Shared) {
        let connection = Arc::new(Mutex::new(Some(Connection {
            stream: Box::pin(response),
            pending: Bytes::new(),
            remaining: length,
            _permit: permit,
            _token: token,
            started: Instant::now(),
            outcome: "canceled",
            span: tracing::Span::current(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        })));
        let audio = Self {
            connection: connection.clone(),
            canceled: Box::pin(stopped.cancelled_owned()),
            idle: None,
            finished: false,
        };
        (audio, connection)
    }
}

impl Stream for Audio {
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.finished {
            return Poll::Ready(None);
        }
        let canceled = this.canceled.as_mut().poll(context).is_ready();
        let mut connection = this
            .connection
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if canceled {
            if let Some(current) = connection.as_mut() {
                current.outcome = "canceled";
            }
            *connection = None;
            this.idle = None;
            this.finished = true;
            return Poll::Ready(Some(Err(io::Error::other("YouTube audio input stopped"))));
        }
        let Some(current) = connection.as_mut() else {
            this.idle = None;
            return Poll::Ready(None);
        };
        if !current.pending.is_empty() {
            return Poll::Ready(Some(Ok(current.take_piece())));
        }
        let result = current.stream.as_mut().poll_next(context);
        if result.is_ready() {
            this.idle = None;
        }
        match result {
            Poll::Pending => {
                let idle = this
                    .idle
                    .get_or_insert_with(|| Box::pin(tokio::time::sleep(Duration::from_secs(10))));
                if idle.as_mut().poll(context).is_pending() {
                    return Poll::Pending;
                }
                current.outcome = "deadline";
                *connection = None;
                this.idle = None;
                this.finished = true;
                Poll::Ready(Some(Err(io::Error::other("YouTube audio input stopped"))))
            }
            Poll::Ready(Some(Ok(bytes)))
                if !bytes.is_empty() && bytes.len() as u64 <= current.remaining =>
            {
                if bytes.len() > MAX_OUTPUT_BYTES {
                    tracing::dispatcher::with_default(&current.dispatch, || {
                        let _span = current.span.enter();
                        tracing::trace!(
                            event = "youtube_audio_frame_split",
                            input_bytes = bytes.len(),
                            output_bytes = MAX_OUTPUT_BYTES
                        );
                    });
                }
                current.pending = bytes;
                Poll::Ready(Some(Ok(current.take_piece())))
            }
            Poll::Ready(None) if current.remaining == 0 => {
                current.outcome = "complete";
                *connection = None;
                this.finished = true;
                Poll::Ready(None)
            }
            Poll::Ready(_) => {
                current.outcome = "invalid_response";
                *connection = None;
                this.finished = true;
                Poll::Ready(Some(Err(io::Error::other(
                    "YouTube audio representation failed",
                ))))
            }
        }
    }
}

impl Connection {
    fn take_piece(&mut self) -> Bytes {
        let bytes = self
            .pending
            .split_to(self.pending.len().min(MAX_OUTPUT_BYTES));
        self.remaining -= bytes.len() as u64;
        if self.remaining == 0 {
            self.outcome = "complete";
        }
        bytes
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        tracing::dispatcher::with_default(&self.dispatch, || {
            let _span = self.span.enter();
            tracing::info!(
                event = "youtube_audio_complete",
                outcome = self.outcome,
                elapsed_ms = self.started.elapsed().as_millis() as u64
            );
        });
    }
}
