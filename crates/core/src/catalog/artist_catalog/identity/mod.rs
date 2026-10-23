mod choices;
mod groups;
mod receipts;
mod reset;
mod types;

#[cfg(test)]
#[path = "tests/retirement_work.rs"]
mod retirement_work;

pub(crate) use groups::push_unrepresented_credit;
pub use types::*;

use crate::Result;
use sqlx::{QueryBuilder, Sqlite, Transaction};

pub(crate) async fn retire_changed(
    tx: &mut Transaction<'_, Sqlite>,
    mut targets: QueryBuilder<'static, Sqlite>,
) -> Result<()> {
    targets.push("UPDATE recording_artist_identity_choices AS choice INDEXED BY sqlite_autoindex_recording_artist_identity_choices_1
        SET retired=1,revision=revision+1 WHERE retired=0 AND association_id IS NOT NULL
        AND (choice.user_id,choice.recording_id) IN (SELECT t.user_id,t.recording_id FROM targets t)
        AND EXISTS (SELECT 1 FROM recordings r
            JOIN account_recording_credits credit ON credit.recording_id=r.id AND credit.user_id=choice.user_id
            LEFT JOIN personal_recording_metadata personal ON personal.recording_id=r.id AND personal.user_id=choice.user_id
            WHERE r.id=choice.recording_id AND (credit.artist_id IS NOT choice.artist_id
                OR personal.artist_operation_id IS NOT choice.artist_operation_id
                OR r.metadata_observation_id IS NOT choice.observation_id))");
    targets.build().execute(&mut **tx).await?;
    Ok(())
}
