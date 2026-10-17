use sqlx::{QueryBuilder, Sqlite};

pub(crate) fn push_source_choice(query: &mut QueryBuilder<'_, Sqlite>, account: &str) {
    query.push("(NOT EXISTS(SELECT 1 FROM recording_source_choices choice
        WHERE choice.user_id=")
        .push_bind(account.to_owned())
        .push(" AND choice.recording_id=s.recording_id AND choice.binding_id IS NOT NULL)
        OR (s.provider='plugin' AND EXISTS(SELECT 1 FROM provider_sources remote
            JOIN source_choice_bindings permitted ON permitted.binding_id=remote.binding_id
                AND permitted.recording_id=remote.recording_id
            WHERE remote.source_id=s.id AND remote.recording_id=s.recording_id AND permitted.user_id=")
        .push_bind(account.to_owned())
        .push(")) OR (s.provider='download' AND EXISTS(SELECT 1 FROM download_files file
            JOIN download_file_origins origin ON origin.file_id=file.id
            JOIN source_choice_bindings permitted ON permitted.binding_id=origin.binding_id
                AND permitted.recording_id=origin.recording_id AND permitted.user_id=origin.account_id
            WHERE file.source_id=s.id AND file.recording_id=s.recording_id AND file.account_id=")
        .push_bind(account.to_owned())
        .push(")))");
}
