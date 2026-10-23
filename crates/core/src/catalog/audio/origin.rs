use crate::{Actor, AudioFile, AudioRequest, Error, OpenedAudio, PlaybackSource, Result};
use futures_util::future::BoxFuture;
use std::{any::Any, sync::Arc};

#[derive(Clone)]
pub(crate) struct Located(Arc<dyn Any + Send + Sync>);

impl Located {
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        Self(Arc::new(value))
    }

    pub fn pinned<T: Any>(&self) -> Result<&T> {
        self.0
            .downcast_ref()
            .ok_or_else(|| Error::Internal("Playback state does not match its origin".into()))
    }
}

pub(crate) trait AudioOrigin: Send + Sync {
    fn kind(&self) -> &'static str;

    fn local(&self) -> bool {
        false
    }

    fn locate<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
    ) -> BoxFuture<'a, Result<Option<Located>>>;

    fn still_valid<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
        located: &'a Located,
    ) -> BoxFuture<'a, Result<bool>>;

    fn open_file<'a>(
        &'a self,
        _actor: &'a Actor,
        _source: &'a str,
    ) -> BoxFuture<'a, Result<Option<AudioFile>>> {
        Box::pin(async { Ok(None) })
    }

    fn open<'a>(
        &'a self,
        grant: &'a PlaybackSource,
        request: AudioRequest,
    ) -> BoxFuture<'a, Result<OpenedAudio>> {
        Box::pin(async move {
            let audio = grant.open_file().await?;
            super::open_file_audio(grant, audio, request).await
        })
    }
}

#[derive(Clone)]
pub(crate) struct Origins {
    ordered: Arc<[Arc<dyn AudioOrigin>]>,
    pub provider: Arc<super::provider::ProviderOrigin>,
}

impl Origins {
    pub fn new(
        provider: super::provider::ProviderOrigin,
        podcast: Arc<dyn AudioOrigin>,
        managed: Arc<dyn AudioOrigin>,
        local: Arc<dyn AudioOrigin>,
    ) -> Self {
        let provider = Arc::new(provider);
        Self {
            ordered: Arc::new([provider.clone(), podcast, managed, local]),
            provider,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn AudioOrigin>> {
        self.ordered.iter()
    }

    pub async fn locate(
        &self,
        actor: &Actor,
        source: &str,
    ) -> Result<(Arc<dyn AudioOrigin>, Located)> {
        locate_first(&self.ordered, actor, source).await
    }
}

async fn locate_first(
    ordered: &[Arc<dyn AudioOrigin>],
    actor: &Actor,
    source: &str,
) -> Result<(Arc<dyn AudioOrigin>, Located)> {
    for origin in ordered {
        if let Some(located) = origin.locate(actor, source).await? {
            return Ok((origin.clone(), located));
        }
    }
    Err(Error::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::support;

    struct Claims(&'static str, bool);

    impl AudioOrigin for Claims {
        fn kind(&self) -> &'static str {
            self.0
        }

        fn locate<'a>(
            &'a self,
            _actor: &'a Actor,
            _source: &'a str,
        ) -> BoxFuture<'a, Result<Option<Located>>> {
            Box::pin(async move { Ok(self.1.then(|| Located::new(self.0))) })
        }

        fn still_valid<'a>(
            &'a self,
            _actor: &'a Actor,
            _source: &'a str,
            _located: &'a Located,
        ) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async { Ok(true) })
        }
    }

    #[tokio::test]
    async fn the_first_origin_that_locates_a_source_owns_it() {
        let (_directory, _hito, actor, _library, _music) = support::library("origin-order").await;
        let ordered: Vec<Arc<dyn AudioOrigin>> = vec![
            Arc::new(Claims("first", false)),
            Arc::new(Claims("second", true)),
            Arc::new(Claims("third", true)),
        ];
        let (origin, located) = locate_first(&ordered, &actor, "source").await.unwrap();
        assert_eq!(origin.kind(), "second");
        assert_eq!(located.pinned::<&str>().unwrap(), &"second");
        assert!(matches!(
            locate_first(&ordered[..1], &actor, "source").await,
            Err(Error::NotFound)
        ));
    }

    #[tokio::test]
    async fn production_origins_are_tried_provider_podcast_managed_then_local() {
        let (_directory, hito, _actor, _library, _music) = support::library("origin-kinds").await;
        let kinds: Vec<_> = hito
            .catalog
            .origins
            .iter()
            .map(|origin| origin.kind())
            .collect();
        assert_eq!(kinds, ["provider", "podcast", "managed", "local"]);
    }
}
