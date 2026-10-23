use crate::HostDeviceId;
use sqlx::{QueryBuilder, Sqlite};

pub(in crate::catalog) fn push_selectable_source(
    query: &mut QueryBuilder<'_, Sqlite>,
    account: &str,
) {
    query.push("s.provider='plugin' AND EXISTS(SELECT 1 FROM provider_sources remote
        JOIN provider_items item ON item.id=remote.binding_id AND item.recording_id=remote.recording_id
            AND item.installation_id=remote.installation_id AND item.retired=0
        JOIN provider_source_state snapshot ON snapshot.binding_id=item.id AND snapshot.generation=remote.generation
        JOIN plugin_installations installation ON installation.id=item.installation_id AND installation.user_id=")
        .push_bind(account.to_owned())
        .push(" AND installation.active=1
        JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
        WHERE remote.source_id=s.id AND remote.recording_id=s.recording_id
            AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value='sources'))");
}

pub(in crate::catalog) fn push_selected_source(
    query: &mut QueryBuilder<'_, Sqlite>,
    host: &HostDeviceId,
    account: &str,
) {
    query
        .push("EXISTS(SELECT 1 FROM recording_source_choices choice WHERE choice.user_id=")
        .push_bind(account.to_owned())
        .push(" AND choice.recording_id=s.recording_id AND choice.binding_id IS NOT NULL) AND ");
    push_source_choice(query, account);
    query.push(" AND (s.provider!='download' OR ");
    super::super::availability::push_managed_access(query, host, account);
    query.push(")");
}

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
