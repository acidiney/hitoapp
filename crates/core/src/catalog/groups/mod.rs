//! One card per proven song. A group never changes a recording, its sources or any personal
//! reference; it only decides which recording a list shows for the account. Whether a copy is
//! hidden is decided when a list is read, so a later like, edit, download or lost audio shows the
//! copy again without waiting for a pass.
mod copies;
mod proof;

pub use copies::{RecordingCopies, RecordingCopy, RecordingGroupProof, RecordingSplit};
pub(crate) use proof::group_step;
use sqlx::{QueryBuilder, Sqlite};

/// Personal state a copy can hold: table, account column and a filter template (`{}` is the alias).
const DECISIONS: [(&str, &str, &str); 6] = [
    ("likes", "user_id", "AND {}.liked = 1"),
    ("personal_recording_metadata", "user_id", ""),
    ("recording_source_choices", "user_id", ""),
    ("listening_progress", "user_id", ""),
    ("audiobook_bookmarks", "user_id", ""),
    ("download_intents", "account_id", ""),
];

/// 2 when the recording has ready audio, 1 when a connected service of the account can resolve
/// it, otherwise 0.
fn rank_sql(recording: &str, account: &str) -> String {
    format!(
        "(CASE WHEN EXISTS (SELECT 1 FROM sources ranked WHERE ranked.recording_id = {recording} AND ranked.ready = 1) THEN 2
              WHEN EXISTS (SELECT 1 FROM provider_items item JOIN plugin_installations service ON service.id = item.installation_id
                  WHERE item.recording_id = {recording} AND item.retired = 0 AND service.user_id = {account}) THEN 1 ELSE 0 END)"
    )
}

/// Restricts a list of recordings `r` to the cards: a copy is hidden only while its carrier is
/// readable by the account, the copy holds no personal state the carrier lacks and the carrier's
/// audio is at least as good.
pub(crate) fn push_cards_only(query: &mut QueryBuilder<'_, Sqlite>, account: &str) {
    query
        .push(" AND NOT EXISTS (SELECT 1 FROM recording_group_members grouped WHERE grouped.user_id = ")
        .push_bind(account.to_owned())
        .push(
            " AND grouped.recording_id = r.id AND EXISTS (SELECT 1 FROM recordings carrier
              JOIN library_members readable ON readable.library_id = carrier.library_id AND readable.user_id = grouped.user_id
              WHERE carrier.id = grouped.canonical_id)",
        );
    for (table, column, filter) in DECISIONS {
        let held = filter.replace("{}", "held");
        let carried = filter.replace("{}", "carried");
        query.push(format!(
            " AND NOT EXISTS (SELECT 1 FROM {table} held WHERE held.{column} = grouped.user_id
              AND held.recording_id = grouped.recording_id {held}
              AND NOT EXISTS (SELECT 1 FROM {table} carried WHERE carried.{column} = grouped.user_id
                  AND carried.recording_id = grouped.canonical_id {carried}))"
        ));
    }
    query.push(format!(
        " AND {} <= {})",
        rank_sql("grouped.recording_id", "grouped.user_id"),
        rank_sql("grouped.canonical_id", "grouped.user_id")
    ));
}
