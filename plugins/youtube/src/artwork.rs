mod jpeg;

use crate::{Error, media::Client, recordings};
use axum::{body::Bytes, http::StatusCode};
use futures_util::StreamExt;
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use std::{sync::Arc, time::Duration, time::Instant};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use url::Url;

const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(8);

pub(crate) struct Fetcher {
    client: Result<Client, Error>,
    slots: Arc<Semaphore>,
    stopped: CancellationToken,
}

impl Fetcher {
    pub(crate) fn new(client: Result<Client, Error>) -> Self {
        Self {
            client,
            slots: Arc::new(Semaphore::new(4)),
            stopped: CancellationToken::new(),
        }
    }

    pub(crate) async fn fetch(&self, request: ArtworkRequest) -> Result<Bytes, Error> {
        let started = Instant::now();
        request
            .validate()
            .map_err(|_| Error::InvalidArtworkRequest)?;
        if !recordings::valid_id(&request.provider_id) {
            return Err(Error::InvalidArtworkRequest);
        }
        if request.entity != ArtworkEntity::Recording {
            return Err(Error::NotFound);
        }
        let _slot = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let result = tokio::select! {
            biased;
            _ = self.stopped.cancelled() => Err(Error::Canceled),
            result = tokio::time::timeout(DEADLINE, self.download(&request.provider_id)) => {
                result.unwrap_or(Err(Error::Deadline))
            }
        };
        tracing::debug!(
            event = "youtube_artwork_complete",
            outcome = result
                .as_ref()
                .map_or_else(|error| error.outcome(), |_| "complete"),
            image_bytes = result.as_ref().map_or(0, Bytes::len),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        result
    }

    pub(crate) fn shutdown(&self) {
        self.stopped.cancel();
    }

    async fn download(&self, video: &str) -> Result<Bytes, Error> {
        let client = self.client.as_ref().map_err(|error| *error)?;
        let url = Url::parse(&format!("https://i.ytimg.com/vi/{video}/hqdefault.jpg"))
            .map_err(|_| Error::Unavailable)?;
        let response = client.image(&url).await?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Err(Error::NotFound),
            _ => return Err(Error::Unavailable),
        }
        let headers = response.headers();
        let content_type = single(headers.get_all("content-type"))?
            .and_then(|value| value.split(';').next())
            .map(str::trim);
        if !content_type.is_some_and(|value| value.eq_ignore_ascii_case("image/jpeg"))
            || single(headers.get_all("content-encoding"))?
                .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
            || headers.contains_key("content-range")
        {
            return Err(Error::InvalidMedia);
        }
        let declared = single(headers.get_all("content-length"))?
            .map(|value| value.parse::<usize>().map_err(|_| Error::InvalidMedia))
            .transpose()?;
        if declared.is_some_and(|length| length == 0 || length > MAX_IMAGE_BYTES) {
            return Err(Error::InvalidMedia);
        }
        let mut body = response.into_body();
        let mut image = Vec::with_capacity(declared.unwrap_or(0));
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(|_| Error::Unavailable)?;
            if chunk.len() > MAX_IMAGE_BYTES - image.len() {
                return Err(Error::InvalidMedia);
            }
            image.extend_from_slice(&chunk);
        }
        if declared.is_some_and(|length| length != image.len()) {
            return Err(Error::InvalidMedia);
        }
        jpeg::validate(&image)?;
        Ok(Bytes::from(image))
    }
}

fn single<'a>(
    values: axum::http::header::GetAll<'a, axum::http::HeaderValue>,
) -> Result<Option<&'a str>, Error> {
    let mut values = values.iter();
    let first = values
        .next()
        .map(|value| value.to_str().map_err(|_| Error::InvalidMedia))
        .transpose()?;
    if values.next().is_some() {
        return Err(Error::InvalidMedia);
    }
    Ok(first)
}
