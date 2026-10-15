use crate::HostDeviceId;
use sqlx::{QueryBuilder, Sqlite};

pub(crate) enum SourceAccess {
    Playable,
    LocalFile,
}

pub(crate) fn push_source_access(
    query: &mut QueryBuilder<'_, Sqlite>,
    host: &HostDeviceId,
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
    query.push(
        " AND (s.provider != 'plugin' OR EXISTS (
        SELECT 1 FROM provider_sources ps
        JOIN plugin_installations p ON p.id = ps.installation_id AND p.active = 1
        JOIN plugin_services service ON service.id = p.service_id AND service.active = 1
        WHERE ps.source_id = s.id AND EXISTS (
            SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'sources')))",
    );
}

pub(crate) fn push_available(query: &mut QueryBuilder<'_, Sqlite>, host: &HostDeviceId) {
    query.push("EXISTS (SELECT 1 FROM sources s WHERE s.recording_id = r.id AND ");
    push_source_access(query, host, SourceAccess::Playable);
    query.push(")");
}
