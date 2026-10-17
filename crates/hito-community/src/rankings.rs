use std::{cmp::Reverse, collections::HashSet, sync::Arc};

use crate::{
    Error,
    storage::Storage,
    wire::{RankCandidates, RankedSource, Ranking},
};

#[derive(Clone)]
pub(crate) struct Rankings {
    storage: Arc<Storage>,
}

impl Rankings {
    pub(crate) fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }

    pub(crate) async fn rank(&self, request: RankCandidates) -> Result<Ranking, Error> {
        if request.candidates.is_empty() || request.candidates.len() > 32 {
            return Err(Error::InvalidInput);
        }
        let mut seen = HashSet::new();
        for source in &request.candidates {
            source.validate()?;
            if !seen.insert(source.clone()) {
                return Err(Error::InvalidInput);
            }
        }
        let mut transaction = self.storage.pool.begin().await?;
        let mut ranked = Vec::with_capacity(request.candidates.len());
        for source in request.candidates {
            let successful_reporters = sqlx::query_scalar(
                "SELECT count(DISTINCT reporter_id) FROM outcomes WHERE provider = ? AND recording_id = ?",
            )
            .bind(source.provider.as_str())
            .bind(&source.recording_id)
            .fetch_one(&mut *transaction)
            .await?;
            ranked.push(RankedSource {
                source,
                successful_reporters,
            });
        }
        transaction.commit().await?;
        ranked.sort_by_key(|source| Reverse(source.successful_reporters));
        tracing::debug!(
            event = "community_ranking_read",
            outcome = "ready",
            candidate_count = ranked.len()
        );
        Ok(Ranking { ranked })
    }
}
