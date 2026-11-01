use crate::{Actor, Artwork, Catalog, Error, Result};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest, Capability};
use std::time::Duration;

const ATTEMPTS: usize = 6;
const BUSY_PAUSE: Duration = Duration::from_millis(150);
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG: &[u8] = b"\xff\xd8\xff";

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn featured_artwork(
        &self,
        actor: &Actor,
        installation: &str,
        revision: i64,
        artwork: &str,
    ) -> Result<Artwork> {
        let request = ArtworkRequest {
            entity: ArtworkEntity::Featured,
            provider_id: artwork.into(),
        };
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid featured artwork identity".into()))?;
        actor.check_session(&self.store).await?;
        let operation = self
            .plugins
            .operation(
                actor,
                installation,
                revision,
                &[Capability::Featured, Capability::Artwork],
            )
            .await?;
        for _ in 0..ATTEMPTS {
            if let Some(found) = operation.artwork(&request).await? {
                operation.check_current().await?;
                let signature = if found.content_type == "image/png" {
                    PNG
                } else {
                    JPEG
                };
                if !found.bytes.starts_with(signature) {
                    return Err(Error::Upstream("The provider returned an invalid image"));
                }
                return Ok(Artwork {
                    content_type: found.content_type,
                    bytes: found.bytes,
                });
            }
            tokio::time::sleep(BUSY_PAUSE).await;
        }
        Err(Error::ProviderBusy("Artwork requests are busy. Try again."))
    }
}
