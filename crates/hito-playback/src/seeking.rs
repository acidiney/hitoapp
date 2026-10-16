use crate::{
    Source,
    source::{Body, denied, interrupted},
};
use bytes::Bytes;
use hito_core::{AudioRequest, ByteRange, OpenedAudio};
use std::io::{self, Seek, SeekFrom};

impl Seek for Source {
    #[tracing::instrument(level = "debug", skip_all)]
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.check()?;
        if let Some(Body::Local(file)) = &mut self.body {
            let position = file.seek(position)?;
            self.check()?;
            self.position = position;
            return Ok(position);
        }
        let target = match position {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => self
                .length
                .and_then(|length| length.checked_add_signed(offset)),
        }
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Audio seek is outside the known input",
            )
        })?;
        if target == self.position {
            return Ok(target);
        }
        if !self.seekable {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Audio input does not support seeking",
            ));
        }
        self.open_at(target)
    }
}

impl Source {
    pub(crate) fn open_at(&mut self, target: u64) -> io::Result<u64> {
        let result = self.reopen(target);
        if let Err(error) = &result {
            self.failure = Some(error.kind());
        }
        result
    }

    fn reopen(&mut self, target: u64) -> io::Result<u64> {
        self.body = None;
        self.pending = Bytes::new();
        self.range_end = None;
        if self.length.is_some_and(|length| target >= length) {
            self.position = target;
            return Ok(target);
        }
        let opened = self.runtime.block_on(async {
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => Err(interrupted()),
                _ = self.grant.revoked() => Err(denied()),
                result = self.grant.open(AudioRequest { head: false, range: Some(ByteRange::From { first: target, last: None }) }) => result.map_err(|_| io::Error::other("Audio seek failed")),
            }
        })?;
        let mut audio = match opened {
            OpenedAudio::Content(audio) => audio,
            OpenedAudio::Unsatisfiable {
                length: Some(length),
            } if length == target && self.length.is_none() => {
                self.length = Some(length);
                self.position = target;
                return Ok(target);
            }
            _ => return Err(io::Error::other("Audio seek was not satisfied")),
        };
        if audio.range.as_ref().map_or(0, |range| *range.start()) != target
            || self
                .length
                .zip(audio.total_length)
                .is_some_and(|(before, after)| before != after)
        {
            return Err(io::Error::other("Audio input changed during a seek"));
        }
        self.check()?;
        self.body =
            Some(Body::Remote(Box::new(audio.body.take().ok_or_else(
                || io::Error::other("Audio seek returned no input"),
            )?)));
        self.position = target;
        self.length = self.length.or(audio.total_length);
        self.range_end = audio.range.as_ref().map(|range| *range.end());
        tracing::debug!(position_bytes = target, "Authorized native input seeked");
        Ok(target)
    }
}
