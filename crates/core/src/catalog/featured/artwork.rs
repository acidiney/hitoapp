use crate::{Actor, Artwork, Catalog, Error, Result};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest, Capability};
use std::time::Duration;

const ATTEMPTS: usize = 20;
const MAX_SIDE: u32 = 4096;
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
        if !self.automatic_service_contact_allowed(actor).await? {
            return Err(Error::NotFound);
        }
        if let Some((content_type, bytes)) =
            self.featured_cache.cover(installation, revision, artwork)
        {
            return Ok(Artwork {
                content_type,
                bytes,
            });
        }
        for _ in 0..ATTEMPTS {
            if let Some(found) = operation.artwork(&request).await? {
                operation.check_current().await?;
                let png = found.content_type == "image/png";
                if !found.bytes.starts_with(if png { PNG } else { JPEG })
                    || !reasonable(&found.bytes, png)
                {
                    return Err(Error::Upstream("The provider returned an invalid image"));
                }
                self.featured_cache.keep_cover(
                    installation,
                    revision,
                    artwork,
                    found.content_type,
                    &found.bytes,
                );
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

fn reasonable(bytes: &[u8], png: bool) -> bool {
    let sides = if png {
        png_sides(bytes)
    } else {
        jpeg_sides(bytes)
    };
    sides.is_some_and(|(width, height)| {
        (1..=MAX_SIDE).contains(&width) && (1..=MAX_SIDE).contains(&height)
    })
}

fn png_sides(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    Some((width, height))
}

fn jpeg_sides(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    while at + 4 <= bytes.len() {
        if bytes[at] != 0xff {
            return None;
        }
        let marker = bytes[at + 1];
        if marker == 0xff {
            at += 1;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let height = u16::from_be_bytes(bytes.get(at + 5..at + 7)?.try_into().ok()?);
            let width = u16::from_be_bytes(bytes.get(at + 7..at + 9)?.try_into().ok()?);
            return Some((u32::from(width), u32::from(height)));
        }
        at += 2 + length;
    }
    None
}
