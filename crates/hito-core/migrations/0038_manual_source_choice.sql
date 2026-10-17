ALTER TABLE recording_source_choices ADD COLUMN binding_id TEXT REFERENCES provider_items(id);

CREATE TABLE source_choice_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    source_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision > 0),
    PRIMARY KEY(user_id,request_id)
);
CREATE TRIGGER source_choice_receipt_immutable BEFORE UPDATE ON source_choice_receipts BEGIN
    SELECT RAISE(ABORT, 'Source choice receipts are immutable');
END;
CREATE TRIGGER source_choice_receipt_retained BEFORE DELETE ON source_choice_receipts BEGIN
    SELECT RAISE(ABORT, 'Source choice receipts must be retained');
END;
CREATE TRIGGER recording_source_choice_owner_insert BEFORE INSERT ON recording_source_choices
WHEN new.binding_id IS NOT NULL AND NOT EXISTS(
    SELECT 1 FROM provider_items item JOIN plugin_installations installation ON installation.id=item.installation_id
    WHERE item.id=new.binding_id AND item.recording_id=new.recording_id AND installation.user_id=new.user_id) BEGIN
    SELECT RAISE(ABORT, 'Source choice binding ownership does not match');
END;
CREATE TRIGGER recording_source_choice_owner_update BEFORE UPDATE ON recording_source_choices
WHEN new.binding_id IS NOT NULL AND NOT EXISTS(
    SELECT 1 FROM provider_items item JOIN plugin_installations installation ON installation.id=item.installation_id
    WHERE item.id=new.binding_id AND item.recording_id=new.recording_id AND installation.user_id=new.user_id) BEGIN
    SELECT RAISE(ABORT, 'Source choice binding ownership does not match');
END;

CREATE VIEW source_choice_bindings AS
SELECT installation.user_id,item.recording_id,item.id AS binding_id
FROM provider_items item JOIN plugin_installations installation ON installation.id=item.installation_id
LEFT JOIN recording_source_choices choice ON choice.user_id=installation.user_id AND choice.recording_id=item.recording_id
WHERE choice.binding_id IS NULL OR choice.binding_id=item.id;

CREATE VIEW download_file_origins AS
SELECT file.id AS file_id,file.account_id,file.host_device,file.recording_id,witness.binding_id
FROM download_files file JOIN download_intents witness ON witness.id=(
    SELECT intent.id FROM download_intents intent WHERE intent.file_id=file.id LIMIT 1)
WHERE witness.account_id=file.account_id AND witness.host_device=file.host_device AND witness.recording_id=file.recording_id
    AND NOT EXISTS(SELECT 1 FROM download_intents other WHERE other.file_id=file.id AND (
        other.account_id IS NOT witness.account_id OR other.host_device IS NOT witness.host_device
        OR other.recording_id IS NOT witness.recording_id OR other.library_id IS NOT witness.library_id
        OR other.provider_source_id IS NOT witness.provider_source_id OR other.binding_id IS NOT witness.binding_id
        OR other.binding_revision IS NOT witness.binding_revision OR other.remote_source_id IS NOT witness.remote_source_id
        OR other.installation_id IS NOT witness.installation_id OR other.installation_revision IS NOT witness.installation_revision
        OR other.service_id IS NOT witness.service_id OR other.service_revision IS NOT witness.service_revision
        OR other.credential_digest IS NOT witness.credential_digest OR other.audio_generation IS NOT witness.audio_generation));

DROP VIEW current_download_authority;
CREATE VIEW current_download_authority AS
SELECT i.id FROM download_intents i
JOIN retained_download_intents retained ON retained.id=i.id
JOIN users account ON account.id=i.account_id
JOIN recordings recording ON recording.id=i.recording_id AND recording.library_id=i.library_id
JOIN library_access_grants access ON access.library_id=i.library_id AND access.user_id=i.account_id AND access.id=i.access_grant_id
JOIN sources source ON source.id=i.provider_source_id AND source.recording_id=i.recording_id AND source.ready=1
JOIN provider_sources ps ON ps.source_id=source.id AND ps.recording_id=i.recording_id
    AND ps.binding_id=i.binding_id AND ps.installation_id=i.installation_id AND ps.provider_source_id=i.remote_source_id
JOIN provider_items binding ON binding.id=i.binding_id AND binding.recording_id=i.recording_id
    AND binding.installation_id=i.installation_id AND binding.revision=i.binding_revision AND binding.retired=0
JOIN source_choice_bindings permitted ON permitted.binding_id=binding.id AND permitted.recording_id=i.recording_id AND permitted.user_id=i.account_id
JOIN provider_source_state source_state ON source_state.binding_id=binding.id AND source_state.generation=ps.generation
JOIN plugin_installations installation ON installation.id=i.installation_id AND installation.user_id=i.account_id
    AND installation.active=1 AND installation.revision=i.installation_revision AND installation.service_id=i.service_id
    AND installation.audio_generation=i.audio_generation
JOIN plugin_services service ON service.id=i.service_id AND service.active=1 AND service.revision=i.service_revision
WHERE i.state IN ('queued','running','verifying')
    AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value='sources');
