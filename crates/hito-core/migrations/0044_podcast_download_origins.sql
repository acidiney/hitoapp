PRAGMA defer_foreign_keys = ON;
CREATE TEMP TABLE hito_download_intent_migration_guard (value INTEGER NOT NULL CHECK(value = 0));
DROP VIEW download_file_origins;
DROP VIEW current_download_authority;
DROP VIEW retained_download_intents;
DROP TRIGGER download_receipt_retention_owner;

CREATE TABLE download_intents_next (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    access_grant_id TEXT NOT NULL,
    host_device TEXT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation > 0),
    state TEXT NOT NULL CHECK(state IN ('queued','running','verifying','ready','failed','removed')),
    file_id TEXT REFERENCES download_files(id) ON DELETE SET NULL,
    provider_source_id TEXT REFERENCES provider_sources(source_id),
    binding_id TEXT REFERENCES provider_items(id),
    binding_revision INTEGER CHECK(binding_revision > 0),
    remote_source_id TEXT,
    installation_id TEXT REFERENCES plugin_installations(id),
    installation_revision INTEGER CHECK(installation_revision > 0),
    service_id TEXT REFERENCES plugin_services(id),
    service_revision INTEGER CHECK(service_revision > 0),
    credential_digest TEXT CHECK(length(credential_digest) = 64),
    audio_generation INTEGER CHECK(audio_generation >= 0),
    bytes_received INTEGER NOT NULL DEFAULT 0 CHECK(bytes_received BETWEEN 0 AND 2147483648),
    total_bytes INTEGER CHECK(total_bytes BETWEEN 1 AND 2147483648),
    error TEXT CHECK(error IS NULL OR error IN ('provider_unavailable','transfer_failed','authority_changed','lease_expired','file_unavailable')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    origin_kind TEXT NOT NULL DEFAULT 'provider' CHECK(origin_kind IN ('provider','podcast')),
    podcast_source_id TEXT REFERENCES podcast_sources(source_id),
    podcast_episode_id TEXT REFERENCES podcast_episodes(id),
    podcast_subscription_id TEXT REFERENCES podcast_subscriptions(id),
    podcast_subscription_generation INTEGER CHECK(podcast_subscription_generation > 0),
    podcast_enclosure_revision INTEGER CHECK(podcast_enclosure_revision > 0),
    podcast_enclosure_url TEXT,
    CHECK((origin_kind='provider'
        AND provider_source_id IS NOT NULL AND binding_id IS NOT NULL AND binding_revision IS NOT NULL
        AND remote_source_id IS NOT NULL AND installation_id IS NOT NULL AND installation_revision IS NOT NULL
        AND service_id IS NOT NULL AND service_revision IS NOT NULL AND credential_digest IS NOT NULL
        AND audio_generation IS NOT NULL
        AND podcast_source_id IS NULL AND podcast_episode_id IS NULL AND podcast_subscription_id IS NULL
        AND podcast_subscription_generation IS NULL AND podcast_enclosure_revision IS NULL AND podcast_enclosure_url IS NULL)
        OR (origin_kind='podcast'
        AND provider_source_id IS NULL AND binding_id IS NULL AND binding_revision IS NULL
        AND remote_source_id IS NULL AND installation_id IS NULL AND installation_revision IS NULL
        AND service_id IS NULL AND service_revision IS NULL AND credential_digest IS NULL AND audio_generation IS NULL
        AND podcast_source_id IS NOT NULL AND podcast_episode_id IS NOT NULL AND podcast_subscription_id IS NOT NULL
        AND podcast_subscription_generation IS NOT NULL AND podcast_enclosure_revision IS NOT NULL
        AND podcast_enclosure_url IS NOT NULL AND length(podcast_enclosure_url) BETWEEN 1 AND 4096)),
    CHECK(state != 'ready' OR file_id IS NOT NULL),
    UNIQUE(id,account_id)
);

INSERT INTO download_intents_next(rowid,id,account_id,recording_id,library_id,access_grant_id,host_device,generation,state,file_id,
    provider_source_id,binding_id,binding_revision,remote_source_id,installation_id,installation_revision,service_id,service_revision,
    credential_digest,audio_generation,bytes_received,total_bytes,error,created_at,updated_at)
SELECT rowid,id,account_id,recording_id,library_id,access_grant_id,host_device,generation,state,file_id,
    provider_source_id,binding_id,binding_revision,remote_source_id,installation_id,installation_revision,service_id,service_revision,
    credential_digest,audio_generation,bytes_received,total_bytes,error,created_at,updated_at FROM download_intents;
INSERT INTO hito_download_intent_migration_guard
SELECT 1 WHERE (SELECT count(*) FROM download_intents_next) != (SELECT count(*) FROM download_intents);
INSERT INTO hito_download_intent_migration_guard
SELECT 1 FROM download_intents original LEFT JOIN download_intents_next copied ON copied.rowid = original.rowid AND copied.id IS original.id
WHERE copied.rowid IS NULL LIMIT 1;
DROP TABLE download_intents;
ALTER TABLE download_intents_next RENAME TO download_intents;

CREATE UNIQUE INDEX download_current_intent ON download_intents(account_id,recording_id,host_device) WHERE state != 'removed';
CREATE INDEX download_file_references ON download_intents(file_id,state);
CREATE INDEX download_intent_account ON download_intents(account_id,host_device,state,id);
CREATE INDEX download_intent_queued_host ON download_intents(host_device,id) WHERE state='queued';

CREATE INDEX download_source_reference ON download_intents(provider_source_id);
CREATE INDEX download_intent_binding ON download_intents(binding_id);

CREATE TRIGGER download_file_owner BEFORE UPDATE OF file_id ON download_intents
WHEN new.file_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM download_files f WHERE f.id = new.file_id
    AND f.account_id = new.account_id AND f.recording_id = new.recording_id AND f.host_device = new.host_device AND f.state IN ('staging','ready')) BEGIN
    SELECT RAISE(ABORT, 'Download file ownership does not match');
END;
CREATE TRIGGER download_file_owner_insert BEFORE INSERT ON download_intents
WHEN new.file_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM download_files f WHERE f.id = new.file_id
    AND f.account_id = new.account_id AND f.recording_id = new.recording_id AND f.host_device = new.host_device AND f.state = 'ready') BEGIN
    SELECT RAISE(ABORT, 'Download file ownership does not match');
END;
CREATE TRIGGER download_intent_identity BEFORE UPDATE ON download_intents
WHEN old.id IS NOT new.id OR old.account_id IS NOT new.account_id OR old.recording_id IS NOT new.recording_id
    OR old.library_id IS NOT new.library_id OR old.access_grant_id IS NOT new.access_grant_id OR old.host_device IS NOT new.host_device
    OR old.provider_source_id IS NOT new.provider_source_id OR old.binding_id IS NOT new.binding_id OR old.binding_revision IS NOT new.binding_revision
    OR old.remote_source_id IS NOT new.remote_source_id OR old.installation_id IS NOT new.installation_id
    OR old.installation_revision IS NOT new.installation_revision OR old.service_id IS NOT new.service_id
    OR old.service_revision IS NOT new.service_revision OR old.credential_digest IS NOT new.credential_digest OR old.audio_generation IS NOT new.audio_generation
    OR old.origin_kind IS NOT new.origin_kind OR old.podcast_source_id IS NOT new.podcast_source_id
    OR old.podcast_episode_id IS NOT new.podcast_episode_id OR old.podcast_subscription_id IS NOT new.podcast_subscription_id
    OR old.podcast_subscription_generation IS NOT new.podcast_subscription_generation
    OR old.podcast_enclosure_revision IS NOT new.podcast_enclosure_revision OR old.podcast_enclosure_url IS NOT new.podcast_enclosure_url
    OR (old.state = 'removed' AND new.state != 'removed') BEGIN
    SELECT RAISE(ABORT, 'Download intent identity is immutable');
END;

CREATE TRIGGER download_receipt_retention_owner BEFORE INSERT ON download_receipts
WHEN new.retention_id IS NOT NULL AND NOT EXISTS(
    SELECT 1 FROM download_retentions r JOIN download_intents i ON i.id=new.intent_id
    WHERE r.id=new.retention_id AND r.account_id=new.account_id AND r.account_id=i.account_id
        AND r.host_device=i.host_device AND r.recording_id=i.recording_id AND r.kind='recording') BEGIN
    SELECT RAISE(ABORT, 'Download receipt retention ownership does not match');
END;

CREATE TRIGGER download_terminal_failure_insert AFTER INSERT ON download_intents
WHEN new.state='failed' BEGIN
    INSERT INTO download_terminal_failures(intent_id) VALUES(new.id);
END;
CREATE TRIGGER download_terminal_failure_update AFTER UPDATE OF state ON download_intents
WHEN new.state='failed' AND old.state!='failed' BEGIN
    INSERT INTO download_terminal_failures(intent_id) VALUES(new.id);
END;

CREATE VIEW retained_download_intents AS
SELECT i.id FROM download_intents i WHERE i.state!='removed' AND (
    EXISTS(SELECT 1 FROM download_retentions r WHERE r.kind='recording' AND r.state='active'
        AND r.account_id=i.account_id AND r.host_device=i.host_device AND r.recording_id=i.recording_id
        AND r.access_grant_id=i.access_grant_id)
    OR EXISTS(SELECT 1 FROM playlist_entries e JOIN playlists p ON p.id=e.playlist_id AND p.deleted=0 AND p.user_id=i.account_id
        JOIN download_retentions r ON r.playlist_id=p.id AND r.account_id=p.user_id AND r.host_device=i.host_device
            AND r.kind='playlist' AND r.state='active'
        WHERE e.recording_id=i.recording_id AND e.deleted=0));
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
WHERE i.origin_kind='provider' AND i.state IN ('queued','running','verifying')
    AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value='sources')
UNION ALL
SELECT i.id FROM download_intents i
JOIN retained_download_intents retained ON retained.id=i.id
JOIN users account ON account.id=i.account_id
JOIN recordings recording ON recording.id=i.recording_id AND recording.library_id=i.library_id
JOIN library_access_grants access ON access.library_id=i.library_id AND access.user_id=i.account_id AND access.id=i.access_grant_id
JOIN current_podcast_sources enclosure ON enclosure.source_id=i.podcast_source_id
    AND enclosure.episode_id=i.podcast_episode_id AND enclosure.recording_id=i.recording_id AND enclosure.user_id=i.account_id
    AND enclosure.subscription_id=i.podcast_subscription_id AND enclosure.subscription_generation=i.podcast_subscription_generation
    AND enclosure.enclosure_revision=i.podcast_enclosure_revision AND enclosure.enclosure_url=i.podcast_enclosure_url
WHERE i.origin_kind='podcast' AND i.state IN ('queued','running','verifying')
    AND NOT EXISTS(SELECT 1 FROM recording_source_choices choice
        WHERE choice.user_id=i.account_id AND choice.recording_id=i.recording_id AND choice.binding_id IS NOT NULL);

CREATE VIEW download_file_origins AS
SELECT file.id AS file_id,file.account_id,file.host_device,file.recording_id,witness.binding_id,witness.origin_kind,
    witness.provider_source_id,witness.binding_revision,witness.remote_source_id,witness.installation_id,
    witness.installation_revision,witness.service_id,witness.service_revision,witness.credential_digest,witness.audio_generation,
    witness.podcast_source_id,witness.podcast_episode_id,witness.podcast_subscription_id,witness.podcast_subscription_generation,
    witness.podcast_enclosure_revision,witness.podcast_enclosure_url
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
        OR other.credential_digest IS NOT witness.credential_digest OR other.audio_generation IS NOT witness.audio_generation
        OR other.origin_kind IS NOT witness.origin_kind OR other.podcast_source_id IS NOT witness.podcast_source_id
        OR other.podcast_episode_id IS NOT witness.podcast_episode_id OR other.podcast_subscription_id IS NOT witness.podcast_subscription_id
        OR other.podcast_subscription_generation IS NOT witness.podcast_subscription_generation
        OR other.podcast_enclosure_revision IS NOT witness.podcast_enclosure_revision
        OR other.podcast_enclosure_url IS NOT witness.podcast_enclosure_url));


INSERT INTO hito_download_intent_migration_guard SELECT 1 FROM pragma_foreign_key_check;
DROP TABLE hito_download_intent_migration_guard;
PRAGMA defer_foreign_keys = OFF;
