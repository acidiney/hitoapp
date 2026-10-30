use crate::Result;
use crate::metadata::RecordingScope;
use sqlx::{Sqlite, Transaction};

/// Marks corrections whose recording no longer carries the credit they were made for. Retired
/// corrections are never revived and never create or merge identities; they stay readable so the
/// account can see that they no longer apply.
pub(crate) async fn retire_stale(
    tx: &mut Transaction<'_, Sqlite>,
    scope: &RecordingScope<'_>,
) -> Result<()> {
    let mut unlinks = scope.recordings();
    unlinks.push(
        "INSERT OR IGNORE INTO artist_association_change_retirements(user_id,association_id,reason)
         SELECT assoc.user_id,assoc.id,'credit_changed' FROM recording_artist_associations assoc INDEXED BY recording_artist_association_recording
         WHERE assoc.recording_id IN (SELECT recording_id FROM targets)
            AND (SELECT change.linked FROM recording_artist_association_changes change
                 WHERE change.user_id=assoc.user_id AND change.association_id=assoc.id
                 ORDER BY change.revision DESC LIMIT 1)=0
            AND NOT EXISTS (SELECT 1 FROM account_recording_credits credit
                WHERE credit.user_id=assoc.user_id AND credit.recording_id=assoc.recording_id
                    AND credit.artist_id=assoc.artist_id)",
    );
    let retired = unlinks.build().execute(&mut **tx).await?.rows_affected();
    let mut splits = scope.recordings();
    splits.push(
        "UPDATE artist_identity_change_items SET retired_reason='credit_changed'
         WHERE retired_reason IS NULL AND recording_id IN (SELECT recording_id FROM targets)
            AND NOT EXISTS (SELECT 1 FROM artist_identity_changes change
                JOIN account_recording_credits credit ON credit.user_id=change.user_id
                    AND credit.recording_id=artist_identity_change_items.recording_id
                    AND credit.artist_id=artist_identity_change_items.from_artist_id
                WHERE change.id=artist_identity_change_items.change_id)",
    );
    let items = splits.build().execute(&mut **tx).await?.rows_affected();
    let mut shared = scope.recordings();
    shared.push(
        "UPDATE shared_artist_correction_subjects SET retired_reason=CASE
            WHEN raw_artist IS NOT (SELECT artist FROM recordings r WHERE r.id=shared_artist_correction_subjects.recording_id)
            THEN 'tags_changed' ELSE 'observation_changed' END
         WHERE retired_reason IS NULL AND recording_id IN (SELECT recording_id FROM targets)
            AND (raw_artist IS NOT (SELECT artist FROM recordings r WHERE r.id=shared_artist_correction_subjects.recording_id)
                OR observation_id IS NOT (SELECT metadata_observation_id FROM recordings r WHERE r.id=shared_artist_correction_subjects.recording_id))",
    );
    let shared_retired = shared.build().execute(&mut **tx).await?.rows_affected();
    if retired + items + shared_retired > 0 {
        tracing::debug!(
            unlinks = retired,
            splits = items,
            shared = shared_retired,
            "Artist corrections retired after a credit change"
        );
    }
    Ok(())
}
