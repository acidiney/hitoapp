mod artwork;
mod cache;
mod types;

pub(crate) use cache::FeaturedCache;
pub use types::{FeaturedEntry, FeaturedShelf};

use crate::{Actor, Catalog, Error, Result};
use hito_plugin_sdk::{Capability, FeaturedItem, FeaturedRequest, MAX_FEATURED_ITEMS};
use std::time::{Duration, Instant};
use tracing::Instrument;
use uuid::Uuid;

const READ_BUDGET: Duration = Duration::from_secs(8);
const PROVIDER_BUDGET: Duration = Duration::from_secs(3);
const MAX_PROVIDERS: usize = 4;
const ITEMS_PER_PROVIDER: u16 = 12;

struct Capable {
    installation_id: String,
    revision: i64,
    provider: String,
    artwork: bool,
}

impl Catalog {
    pub async fn featured(&self, actor: &Actor) -> Result<FeaturedShelf> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        let span =
            tracing::debug_span!("featured_shelf", operation = "featured_shelf", %request_id);
        let result = async {
            tokio::time::timeout(READ_BUDGET, self.read_featured(actor))
                .await
                .unwrap_or(Ok(FeaturedShelf::Unavailable))
        }
        .instrument(span)
        .await;
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(FeaturedShelf::outcome)
                .unwrap_or_else(|error| error.code()),
            item_count = match &result {
                Ok(FeaturedShelf::Available { items }) => items.len(),
                _ => 0,
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Featured shelf read completed"
        );
        result
    }

    async fn read_featured(&self, actor: &Actor) -> Result<FeaturedShelf> {
        actor.check_session(&self.store).await?;
        if !self.automatic_service_contact_allowed(actor).await? {
            return Ok(FeaturedShelf::Disabled);
        }
        let capable: Vec<Capable> = self
            .plugins
            .connections(actor)
            .await?
            .installations
            .into_iter()
            .filter(|installation| {
                installation
                    .manifest
                    .capabilities
                    .contains(&Capability::Featured)
            })
            .take(MAX_PROVIDERS)
            .map(|installation| Capable {
                artwork: installation
                    .manifest
                    .capabilities
                    .contains(&Capability::Artwork),
                installation_id: installation.id,
                revision: installation.revision,
                provider: installation.manifest.name,
            })
            .collect();
        if capable.is_empty() {
            return Ok(FeaturedShelf::Unsupported);
        }
        let mut lists = Vec::new();
        for provider in &capable {
            let items = self.provider_featured(actor, provider).await?;
            lists.push(items);
        }
        let items = interleave(&capable, lists);
        Ok(if items.is_empty() {
            FeaturedShelf::Unavailable
        } else {
            FeaturedShelf::Available { items }
        })
    }

    async fn provider_featured(
        &self,
        actor: &Actor,
        provider: &Capable,
    ) -> Result<Vec<FeaturedItem>> {
        if let Some(answer) = self
            .featured_cache
            .get(&provider.installation_id, provider.revision)
        {
            return Ok(answer.unwrap_or_default());
        }
        let request = FeaturedRequest {
            limit: ITEMS_PER_PROVIDER,
        };
        let asked = async {
            let operation = self
                .plugins
                .operation(
                    actor,
                    &provider.installation_id,
                    provider.revision,
                    &[Capability::Featured],
                )
                .await?;
            operation.featured(&request).await
        };
        match tokio::time::timeout(PROVIDER_BUDGET, asked).await {
            Ok(Ok(page)) => {
                self.featured_cache.put(
                    &provider.installation_id,
                    provider.revision,
                    Some(page.items.clone()),
                );
                Ok(page.items)
            }
            Ok(Err(Error::Upstream(_))) | Err(_) => {
                self.featured_cache
                    .put(&provider.installation_id, provider.revision, None);
                Ok(Vec::new())
            }
            Ok(Err(Error::ProviderBusy(_) | Error::NotFound | Error::Conflict(_))) => {
                Ok(Vec::new())
            }
            Ok(Err(error)) => Err(error),
        }
    }

    async fn automatic_service_contact_allowed(&self, actor: &Actor) -> Result<bool> {
        let stored: Option<bool> = sqlx::query_scalar(
            "SELECT service_search FROM account_metadata_automation WHERE user_id=?",
        )
        .bind(actor.id())
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(stored.unwrap_or(true))
    }
}

fn interleave(capable: &[Capable], lists: Vec<Vec<FeaturedItem>>) -> Vec<FeaturedEntry> {
    let mut lists: Vec<_> = lists.into_iter().map(|items| items.into_iter()).collect();
    let mut entries = Vec::new();
    while entries.len() < usize::from(MAX_FEATURED_ITEMS) {
        let before = entries.len();
        for (provider, items) in capable.iter().zip(lists.iter_mut()) {
            if entries.len() == usize::from(MAX_FEATURED_ITEMS) {
                break;
            }
            if let Some(item) = items.next() {
                entries.push(FeaturedEntry {
                    installation_id: provider.installation_id.clone(),
                    installation_revision: provider.revision,
                    provider: provider.provider.clone(),
                    id: item.id,
                    title: item.title,
                    subtitle: item.subtitle,
                    kind: item.kind,
                    reference: item.reference,
                    artwork_id: item.artwork_id.filter(|_| provider.artwork),
                    item_count: item.item_count,
                });
            }
        }
        if entries.len() == before {
            break;
        }
    }
    entries
}
