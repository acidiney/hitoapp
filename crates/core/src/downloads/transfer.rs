use super::{Downloads, guard::Guard, lease::Lease};
use crate::{
    AudioContent, AudioRequest, AudioStream, AudioValidators, ByteRange, Error, OpenedAudio,
    Result,
    library_storage::{MAX_MANAGED_BYTES, PendingManagedFile},
};

const MAX_RESUMES_PER_ATTEMPT: u32 = 3;

pub(super) struct Representation {
    total: Option<u64>,
    validators: AudioValidators,
    ranges: bool,
}

impl Representation {
    pub(super) fn admit(mut content: Box<AudioContent>) -> Result<(AudioStream, Self)> {
        if content.range.is_some()
            || content
                .length
                .is_some_and(|length| length == 0 || length > MAX_MANAGED_BYTES)
        {
            return Err(Error::Upstream("The download audio exceeds its size limit"));
        }
        let body = content
            .body
            .take()
            .ok_or(Error::Upstream("The download service did not return audio"))?;
        let representation = Self {
            total: content.length,
            validators: content.validators,
            ranges: content.seekable && content.length.is_some(),
        };
        Ok((body, representation))
    }

    pub(super) fn total(&self) -> Option<u64> {
        self.total
    }

    fn continued_by(&self, content: &AudioContent, offset: u64) -> bool {
        let Some(range) = &content.range else {
            return false;
        };
        content.body.is_some()
            && *range.start() == offset
            && range.end().checked_add(1) == self.total
            && content.total_length == self.total
            && self.validators.still_describes(&content.validators)
    }
}

enum Next {
    Continued(AudioStream),
    Restarted(AudioStream, Representation),
}

impl Downloads {
    pub(super) async fn copy_audio(
        &self,
        lease: &Lease,
        guard: &mut Guard<'_>,
        pending: &mut PendingManagedFile,
        mut body: AudioStream,
        mut representation: Representation,
    ) -> Result<Option<()>> {
        let mut resumes = 0;
        loop {
            let Some(outcome) = drain(guard, pending, &mut body).await? else {
                return Ok(None);
            };
            match outcome {
                Ok(()) => {
                    if guard.total.is_some_and(|total| total != guard.bytes) {
                        return Err(Error::Upstream(
                            "The download audio ended before its declared length",
                        ));
                    }
                    return Ok(Some(()));
                }
                Err(error)
                    if matches!(error, Error::Upstream(_))
                        && representation.ranges
                        && pending.length() > 0
                        && resumes < MAX_RESUMES_PER_ATTEMPT =>
                {
                    resumes += 1;
                    drop(body);
                    let offset = pending.length();
                    let Some(next) = self.reopen(lease, guard, &representation, offset).await?
                    else {
                        return Ok(None);
                    };
                    match next {
                        Next::Continued(stream) => {
                            tracing::debug!(
                                job_id = %lease.id,
                                event = "download_transfer_resumed",
                                outcome = "continued",
                                offset,
                                resume = resumes,
                                "Download transfer continued from the received bytes"
                            );
                            body = stream;
                        }
                        Next::Restarted(stream, fresh) => {
                            tracing::debug!(
                                job_id = %lease.id,
                                event = "download_transfer_resumed",
                                outcome = "restarted",
                                offset,
                                resume = resumes,
                                "Download transfer restarted from the first byte"
                            );
                            let Some(()) = guard.wait(pending.rewind()).await? else {
                                return Ok(None);
                            };
                            guard.bytes = 0;
                            guard.total = fresh.total();
                            representation = fresh;
                            body = stream;
                        }
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn reopen(
        &self,
        lease: &Lease,
        guard: &mut Guard<'_>,
        representation: &Representation,
        offset: u64,
    ) -> Result<Option<Next>> {
        let ranged = AudioRequest {
            head: false,
            range: Some(ByteRange::From {
                first: offset,
                last: None,
            }),
        };
        let Some(opened) = guard.wait(self.open_origin(lease, ranged)).await? else {
            return Ok(None);
        };
        let full = match opened {
            OpenedAudio::Content(mut content) if representation.continued_by(&content, offset) => {
                let stream = content
                    .body
                    .take()
                    .ok_or(Error::Upstream("The download service did not return audio"))?;
                return Ok(Some(Next::Continued(stream)));
            }
            OpenedAudio::Content(content) if content.range.is_none() && content.body.is_some() => {
                content
            }
            _ => {
                let Some(opened) = guard
                    .wait(self.open_origin(lease, AudioRequest::default()))
                    .await?
                else {
                    return Ok(None);
                };
                let OpenedAudio::Content(content) = opened else {
                    return Err(Error::Upstream(
                        "The download service did not return complete audio",
                    ));
                };
                content
            }
        };
        let (stream, fresh) = Representation::admit(full)?;
        Ok(Some(Next::Restarted(stream, fresh)))
    }
}

async fn drain(
    guard: &mut Guard<'_>,
    pending: &mut PendingManagedFile,
    body: &mut AudioStream,
) -> Result<Option<Result<()>>> {
    loop {
        let Some(read) = guard
            .wait(async { Ok::<_, Error>(body.chunk().await) })
            .await?
        else {
            return Ok(None);
        };
        let chunk = match read {
            Ok(Some(chunk)) => chunk,
            Ok(None) => return Ok(Some(Ok(()))),
            Err(error) => return Ok(Some(Err(error))),
        };
        if chunk.len() as u64 > MAX_MANAGED_BYTES.saturating_sub(guard.bytes) {
            return Err(Error::Upstream("The download audio exceeds its size limit"));
        }
        if guard.wait(pending.write_chunk(&chunk)).await?.is_none() {
            return Ok(None);
        }
        guard.bytes += chunk.len() as u64;
    }
}
