use super::{Lease, Podcasts, stored_number};
use crate::{
    Error, Result,
    podcasts::feed::{self, FeedSnapshot},
    remote_http::ConditionalValidators,
};

const MAX_FEED_BYTES: usize = 8 * 1024 * 1024;

pub(super) enum Content {
    Modified(FeedSnapshot),
    NotModified,
}

pub(super) struct ReceivedFeed {
    pub(super) content: Content,
    pub(super) validators: ConditionalValidators,
    pub(super) permanent_url: Option<String>,
}

impl Podcasts {
    pub(super) async fn receive(&self, lease: &Lease) -> Result<ReceivedFeed> {
        lease.check(&self.store).await?;
        let previous = lease.validators();
        let remote = self.remote.get(&lease.fetch_url, previous.clone()).await?;
        let mut response = remote.response;
        let mut validators = ConditionalValidators::response(&remote.final_url, response.headers());
        let permanent_url = remote
            .permanent_redirect
            .then(|| remote.final_url.to_string());
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            if lease.content_revision == 0
                || !remote.conditional
                || !previous.applies_to(&remote.final_url)
            {
                return Err(Error::Upstream(
                    "The podcast feed returned an unrequested conditional response",
                ));
            }
            lease.check(&self.store).await?;
            validators.retain_missing(&previous);
            return Ok(ReceivedFeed {
                content: Content::NotModified,
                validators,
                permanent_url,
            });
        }
        if response.status() != reqwest::StatusCode::OK {
            return Err(Error::Upstream("The podcast feed is unavailable"));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_FEED_BYTES as u64)
        {
            return Err(Error::Upstream("The podcast feed exceeds its size limit"));
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
        {
            return Err(Error::Upstream(
                "The podcast feed uses an unsupported encoding",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Upstream("The podcast feed could not be read"))?
        {
            if chunk.len() > MAX_FEED_BYTES.saturating_sub(bytes.len()) {
                return Err(Error::Upstream("The podcast feed exceeds its size limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        lease.check(&self.store).await?;
        let snapshot = feed::parse(&bytes, &remote.final_url)?;
        for episode in &snapshot.episodes {
            stored_number(episode.duration_ms.unwrap_or(0))?;
            episode.length.map(stored_number).transpose()?;
        }
        Ok(ReceivedFeed {
            content: Content::Modified(snapshot),
            validators,
            permanent_url,
        })
    }
}
