use crate::HostDeviceId;
use sqlx::{QueryBuilder, Sqlite};

pub(crate) enum SourceAccess {
    Playable,
    LocalFile,
}

pub(crate) fn push_source_access(
    query: &mut QueryBuilder<'_, Sqlite>,
    host: &HostDeviceId,
    account: &str,
    access: SourceAccess,
) {
    query.push("s.ready = 1 AND ");
    match access {
        SourceAccess::Playable => {
            query
                .push("(s.device_id IS NULL OR s.device_id = ")
                .push_bind(host.as_str().to_owned())
                .push(")");
        }
        SourceAccess::LocalFile => {
            query
                .push("s.device_id = ")
                .push_bind(host.as_str().to_owned());
        }
    }
    query.push(
        " AND (s.provider != 'local' OR EXISTS (
        SELECT 1 FROM local_files file JOIN music_folders folder ON folder.id = file.folder_id
        WHERE file.source_id = s.id AND folder.connected = 1 AND folder.bound = 1))",
    );
    query.push(" AND (s.provider != 'download' OR ");
    push_managed_access(query, host, account);
    query.push(")");
    query.push(
        " AND (s.provider != 'plugin' OR EXISTS (
        SELECT 1 FROM provider_sources ps
        JOIN provider_items item ON item.id = ps.binding_id AND item.retired = 0
        JOIN provider_source_state state ON state.binding_id = item.id AND state.generation = ps.generation
        JOIN plugin_installations p ON p.id = ps.installation_id AND p.active = 1
        JOIN plugin_services service ON service.id = p.service_id AND service.active = 1
        WHERE ps.source_id = s.id AND p.user_id = ",
    ).push_bind(account.to_owned()).push(" AND EXISTS (
            SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'sources')))");
}

pub(super) fn push_managed_access(
    query: &mut QueryBuilder<'_, Sqlite>,
    host: &HostDeviceId,
    account: &str,
) {
    query
        .push(
            "EXISTS (SELECT 1 FROM download_files file WHERE file.source_id = s.id
        AND file.recording_id = s.recording_id AND file.state = 'ready' AND file.host_device = ",
        )
        .push_bind(host.as_str().to_owned())
        .push(" AND file.account_id = ")
        .push_bind(account.to_owned())
        .push(
            " AND EXISTS (SELECT 1 FROM download_intents intent
        JOIN library_access_grants access ON access.library_id = intent.library_id
            AND access.user_id = intent.account_id AND access.id = intent.access_grant_id
        WHERE intent.file_id = file.id AND intent.account_id = file.account_id
            AND intent.recording_id = file.recording_id AND intent.host_device = file.host_device
            AND intent.state = 'ready'))",
        );
}

pub(crate) fn push_available(
    query: &mut QueryBuilder<'_, Sqlite>,
    host: &HostDeviceId,
    account: &str,
) {
    query.push("EXISTS (SELECT 1 FROM sources s WHERE s.recording_id = r.id AND ");
    push_source_access(query, host, account, SourceAccess::Playable);
    query.push(")");
}
