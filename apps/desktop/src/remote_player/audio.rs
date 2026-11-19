use super::server::{Server, failure};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use hito_core::{
    AudioFile, AudioRequest, BeginPlaybackLease, ByteRange, CandidatesEnd, Error, PcmOpening,
    PlaybackLease, Result, Source,
};
use hito_playback::seam::{AudioAccess, Call, Candidates, Chunks, Content, Grant, Opened};
use reqwest::{Method, StatusCode, header};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{runtime::Handle, sync::watch, task::JoinHandle};

const LEASE_CHECK: Duration = Duration::from_secs(2);
const MAX_ATTEMPTS: u64 = 6;

pub struct RemoteCandidates {
    server: Arc<Server>,
    recording: String,
    waiting: VecDeque<Source>,
    offered: u64,
    ended: Option<CandidatesEnd>,
}

impl RemoteCandidates {
    pub fn new(server: Arc<Server>, recording: &str, sources: Vec<Source>) -> Self {
        Self {
            server,
            recording: recording.to_owned(),
            waiting: sources.into_iter().filter(|source| source.ready).collect(),
            offered: 0,
            ended: None,
        }
    }
}

impl Candidates for RemoteCandidates {
    fn ended(&self) -> Option<CandidatesEnd> {
        self.ended
    }

    fn next(&mut self) -> Call<'_, Option<Grant>> {
        Box::pin(async move {
            loop {
                if self.offered >= MAX_ATTEMPTS {
                    self.ended = Some(CandidatesEnd::AttemptLimit);
                    return Ok(None);
                }
                let Some(source) = self.waiting.pop_front() else {
                    self.ended = Some(CandidatesEnd::Exhausted);
                    return Ok(None);
                };
                let request = self
                    .server
                    .request(Method::POST, &["playback", "leases"])?
                    .json(&BeginPlaybackLease {
                        recording_id: self.recording.clone(),
                        source_id: source.id.clone(),
                        client: Some(self.server.lease_tag().to_owned()),
                        request_id: Some(uuid::Uuid::new_v4()),
                    });
                match self.server.json::<PlaybackLease>(request).await {
                    Ok(lease) => {
                        self.offered += 1;
                        tracing::debug!(
                            event = "remote_lease_begun",
                            attempt = self.offered,
                            "The server lent this computer a playback source"
                        );
                        return Ok(Some(Grant::new(RemoteAudio::start(
                            self.server.clone(),
                            lease,
                            source.id,
                        ))));
                    }
                    Err(error @ (Error::Unauthorized | Error::Locked)) => return Err(error),
                    Err(error) => {
                        self.offered += 1;
                        tracing::debug!(
                            event = "remote_lease_refused",
                            error_kind = error.code(),
                            attempt = self.offered,
                            "The server refused one source; trying the next"
                        );
                    }
                }
            }
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Standing {
    Current,
    Lost,
}

pub struct RemoteAudio {
    server: Arc<Server>,
    lease: PlaybackLease,
    source_id: String,
    standing: watch::Receiver<Standing>,
    watching: JoinHandle<()>,
    runtime: Handle,
}

impl RemoteAudio {
    fn start(server: Arc<Server>, lease: PlaybackLease, source_id: String) -> Self {
        let (report, standing) = watch::channel(Standing::Current);
        let watching = tokio::spawn(watch_lease(server.clone(), lease.id, report));
        Self {
            server,
            lease,
            source_id,
            standing,
            watching,
            runtime: Handle::current(),
        }
    }

    fn lost(&self) -> bool {
        *self.standing.borrow() == Standing::Lost
    }
}

async fn watch_lease(server: Arc<Server>, lease: uuid::Uuid, report: watch::Sender<Standing>) {
    let id = lease.to_string();
    loop {
        tokio::time::sleep(LEASE_CHECK).await;
        let Ok(request) = server.request(Method::GET, &["playback", "leases", &id]) else {
            report.send_replace(Standing::Lost);
            return;
        };
        match server.send(request).await {
            Ok(_) => (),
            Err(
                error @ (Error::NotFound | Error::Unauthorized | Error::Forbidden | Error::Locked),
            ) => {
                tracing::info!(
                    event = "remote_lease_lost",
                    error_kind = error.code(),
                    "The server withdrew this computer's playback source"
                );
                report.send_replace(Standing::Lost);
                return;
            }
            Err(error) => tracing::debug!(
                event = "remote_lease_unchecked",
                error_kind = error.code(),
                "The playback source could not be checked; keeping it for now"
            ),
        }
    }
}

impl Drop for RemoteAudio {
    fn drop(&mut self) {
        self.watching.abort();
        let server = self.server.clone();
        let id = self.lease.id.to_string();
        self.runtime.spawn(async move {
            let Ok(request) = server.request(Method::DELETE, &["playback", "leases", &id]) else {
                return;
            };
            let released = server.send(request).await.is_ok();
            tracing::debug!(
                event = "remote_lease_released",
                released,
                "This computer returned its playback source"
            );
        });
    }
}

type ByteStream = Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>;

struct RemoteChunks(Mutex<ByteStream>);

impl Chunks for RemoteChunks {
    fn chunk(&mut self) -> Call<'_, Option<Bytes>> {
        Box::pin(async move {
            let stream = self
                .0
                .get_mut()
                .map_err(|_| Error::Upstream("Audio input failed"))?;
            match stream.next().await {
                Some(Ok(bytes)) => Ok(Some(bytes)),
                Some(Err(_)) => Err(Error::Upstream("The server stopped sending audio")),
                None => Ok(None),
            }
        })
    }
}

fn range_header(range: ByteRange) -> String {
    match range {
        ByteRange::From { first, last: None } => format!("bytes={first}-"),
        ByteRange::From {
            first,
            last: Some(last),
        } => format!("bytes={first}-{last}"),
        ByteRange::Suffix { length } => format!("bytes=-{length}"),
    }
}

fn content_range(value: &str) -> (Option<std::ops::RangeInclusive<u64>>, Option<u64>) {
    let Some(value) = value.strip_prefix("bytes ") else {
        return (None, None);
    };
    let (span, total) = value.split_once('/').unwrap_or((value, "*"));
    let total = total.parse().ok();
    let span = span
        .split_once('-')
        .and_then(|(first, last)| Some(first.parse().ok()?..=last.parse().ok()?));
    (span, total)
}

impl AudioAccess for RemoteAudio {
    fn source_id(&self) -> &str {
        &self.source_id
    }

    fn is_local(&self) -> bool {
        false
    }

    fn check(&self) -> Call<'_, ()> {
        Box::pin(async move {
            if self.lost() {
                return Err(Error::Forbidden);
            }
            Ok(())
        })
    }

    fn revoked(&self) -> Pin<Box<dyn Future<Output = Error> + Send + '_>> {
        let mut standing = self.standing.clone();
        Box::pin(async move {
            let _ = standing
                .wait_for(|standing| *standing == Standing::Lost)
                .await;
            Error::Forbidden
        })
    }

    fn open_file(&self) -> Call<'_, AudioFile> {
        Box::pin(async { Err(Error::UnsupportedAudio) })
    }

    fn open(&self, request: AudioRequest) -> Call<'_, Opened> {
        Box::pin(async move {
            if self.lost() {
                return Err(Error::Forbidden);
            }
            let method = if request.head {
                Method::HEAD
            } else {
                Method::GET
            };
            let id = self.lease.id.to_string();
            let mut builder = self
                .server
                .request(method, &["playback", "leases", &id, "audio"])?;
            if let Some(range) = request.range {
                builder = builder.header(header::RANGE, range_header(range));
            }
            let response = builder
                .send()
                .await
                .map_err(|_| Error::Upstream("The server could not be reached"))?;
            let headers = response.headers();
            let (span, total) = headers
                .get(header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .map(content_range)
                .unwrap_or((None, None));
            match response.status() {
                StatusCode::RANGE_NOT_SATISFIABLE => Ok(Opened::Unsatisfiable { length: total }),
                StatusCode::OK | StatusCode::PARTIAL_CONTENT => {
                    let seekable = headers
                        .get(header::ACCEPT_RANGES)
                        .is_some_and(|value| value == "bytes");
                    let total_length = if response.status() == StatusCode::OK {
                        response.content_length()
                    } else {
                        total
                    };
                    let stream: ByteStream = Box::pin(response.bytes_stream());
                    Ok(Opened::Content(Content {
                        total_length,
                        range: span,
                        seekable,
                        body: Box::new(RemoteChunks(Mutex::new(stream))),
                    }))
                }
                _ => Err(failure(response).await),
            }
        })
    }

    fn begin_pcm(&self, _start: Duration) -> Result<PcmOpening> {
        Err(Error::UnsupportedAudio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_asked_for_the_way_the_server_reads_them() {
        assert_eq!(
            range_header(ByteRange::From {
                first: 10,
                last: None
            }),
            "bytes=10-"
        );
        assert_eq!(
            range_header(ByteRange::From {
                first: 10,
                last: Some(19)
            }),
            "bytes=10-19"
        );
        assert_eq!(range_header(ByteRange::Suffix { length: 5 }), "bytes=-5");
    }

    #[test]
    fn content_ranges_give_the_span_and_the_total_when_known() {
        assert_eq!(content_range("bytes 10-19/100"), (Some(10..=19), Some(100)));
        assert_eq!(content_range("bytes 10-19/*"), (Some(10..=19), None));
        assert_eq!(content_range("bytes */100"), (None, Some(100)));
        assert_eq!(content_range("items 1-2/3"), (None, None));
    }
}
