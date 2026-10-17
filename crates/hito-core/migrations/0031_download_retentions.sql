CREATE TABLE download_retentions (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES users(id),
    host_device TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('recording','playlist')),
    recording_id TEXT REFERENCES recordings(id),
    playlist_id TEXT REFERENCES playlists(id),
    access_grant_id TEXT,
    transfer_id TEXT REFERENCES download_intents(id),
    state TEXT NOT NULL CHECK(state IN ('active','removed')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((kind='recording' AND recording_id IS NOT NULL AND playlist_id IS NULL AND access_grant_id IS NOT NULL AND transfer_id IS NOT NULL)
        OR (kind='playlist' AND playlist_id IS NOT NULL AND recording_id IS NULL AND access_grant_id IS NULL AND transfer_id IS NULL)),
    UNIQUE(id,account_id),
    UNIQUE(id,host_device)
);
INSERT INTO download_retentions(id,account_id,host_device,kind,recording_id,access_grant_id,transfer_id,state,created_at,updated_at)
SELECT id,account_id,host_device,'recording',recording_id,access_grant_id,id,
    CASE WHEN state='removed' THEN 'removed' ELSE 'active' END,created_at,updated_at FROM download_intents;
CREATE UNIQUE INDEX download_recording_retention ON download_retentions(account_id,recording_id,host_device) WHERE kind='recording' AND state='active';
CREATE UNIQUE INDEX download_playlist_retention ON download_retentions(account_id,playlist_id,host_device) WHERE kind='playlist' AND state='active';
CREATE INDEX download_playlist_subscribers ON download_retentions(playlist_id,id) WHERE kind='playlist' AND state='active';
CREATE INDEX download_retention_recording ON download_retentions(recording_id,account_id,host_device,access_grant_id) WHERE kind='recording' AND state='active';
CREATE TRIGGER download_retention_identity BEFORE UPDATE ON download_retentions
WHEN old.id IS NOT new.id OR old.account_id IS NOT new.account_id OR old.host_device IS NOT new.host_device
    OR old.kind IS NOT new.kind OR old.recording_id IS NOT new.recording_id OR old.playlist_id IS NOT new.playlist_id
    OR old.access_grant_id IS NOT new.access_grant_id OR old.transfer_id IS NOT new.transfer_id OR old.created_at IS NOT new.created_at
    OR (old.state='removed' AND new.state!='removed') BEGIN
    SELECT RAISE(ABORT, 'Download retention identity is immutable');
END;
CREATE TRIGGER download_retention_retained BEFORE DELETE ON download_retentions BEGIN
    SELECT RAISE(ABORT, 'Download retention identities must be retained');
END;
ALTER TABLE download_receipts ADD COLUMN retention_id TEXT REFERENCES download_retentions(id);
CREATE TRIGGER download_receipt_retention_owner BEFORE INSERT ON download_receipts
WHEN new.retention_id IS NOT NULL AND NOT EXISTS(
    SELECT 1 FROM download_retentions r JOIN download_intents i ON i.id=new.intent_id
    WHERE r.id=new.retention_id AND r.account_id=new.account_id AND r.account_id=i.account_id
        AND r.host_device=i.host_device AND r.recording_id=i.recording_id AND r.kind='recording') BEGIN
    SELECT RAISE(ABORT, 'Download receipt retention ownership does not match');
END;

CREATE TABLE download_playlist_receipts (
    account_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL CHECK(length(request_digest)=64),
    retention_id TEXT NOT NULL,
    PRIMARY KEY(account_id,request_id),
    FOREIGN KEY(retention_id,account_id) REFERENCES download_retentions(id,account_id)
);
CREATE TRIGGER download_playlist_receipt_immutable BEFORE UPDATE ON download_playlist_receipts BEGIN
    SELECT RAISE(ABORT, 'Playlist download receipts are immutable');
END;
CREATE TRIGGER download_playlist_receipt_retained BEFORE DELETE ON download_playlist_receipts BEGIN
    SELECT RAISE(ABORT, 'Playlist download receipts must be retained');
END;
CREATE TABLE download_playlist_changes (
    playlist_id TEXT PRIMARY KEY REFERENCES playlists(id),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    dirty INTEGER NOT NULL DEFAULT 0 CHECK(dirty IN (0,1)),
    cursor TEXT,
    retirement_cursor TEXT,
    roots_done INTEGER NOT NULL DEFAULT 0 CHECK(roots_done IN (0,1)),
    retirements_done INTEGER NOT NULL DEFAULT 0 CHECK(retirements_done IN (0,1)),
    next_dispatch_at INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX download_playlist_dispatch ON download_playlist_changes(next_dispatch_at,playlist_id) WHERE dirty=1;
CREATE TABLE download_playlist_work (
    retention_id TEXT PRIMARY KEY,
    host_device TEXT NOT NULL,
    desired_revision INTEGER NOT NULL CHECK(desired_revision>0),
    reconciled_revision INTEGER NOT NULL DEFAULT 0 CHECK(reconciled_revision>=0),
    scan_revision INTEGER NOT NULL DEFAULT 0 CHECK(scan_revision>=0),
    phase TEXT NOT NULL DEFAULT 'scan' CHECK(phase IN ('scan','prune','idle')),
    cursor TEXT,
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','idle')),
    next_attempt_at INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY(retention_id,host_device) REFERENCES download_retentions(id,host_device)
);
CREATE INDEX download_playlist_due ON download_playlist_work(host_device,next_attempt_at,retention_id) WHERE status='queued';
CREATE TABLE download_playlist_members (
    retention_id TEXT NOT NULL REFERENCES download_retentions(id),
    entry_id TEXT NOT NULL,
    recording_id TEXT REFERENCES recordings(id),
    seen_revision INTEGER NOT NULL CHECK(seen_revision>0),
    PRIMARY KEY(retention_id,entry_id)
);
CREATE INDEX download_playlist_member_revision ON download_playlist_members(retention_id,seen_revision,entry_id);
CREATE INDEX playlist_download_scan ON playlist_entries(playlist_id,deleted,id);
CREATE TABLE download_retirement_work (
    transfer_id TEXT PRIMARY KEY REFERENCES download_intents(id),
    host_device TEXT NOT NULL,
    playlist_id TEXT NOT NULL REFERENCES download_playlist_changes(playlist_id),
    next_check_at INTEGER NOT NULL
);
CREATE INDEX download_retirement_due ON download_retirement_work(host_device,next_check_at,transfer_id);
CREATE INDEX download_retirement_playlist ON download_retirement_work(playlist_id,transfer_id);

CREATE VIEW retained_download_intents AS
SELECT i.id FROM download_intents i WHERE i.state!='removed' AND (
    EXISTS(SELECT 1 FROM download_retentions r WHERE r.kind='recording' AND r.state='active'
        AND r.account_id=i.account_id AND r.host_device=i.host_device AND r.recording_id=i.recording_id
        AND r.access_grant_id=i.access_grant_id)
    OR EXISTS(SELECT 1 FROM playlist_entries e JOIN playlists p ON p.id=e.playlist_id AND p.deleted=0 AND p.user_id=i.account_id
        JOIN download_retentions r ON r.playlist_id=p.id AND r.account_id=p.user_id AND r.host_device=i.host_device
            AND r.kind='playlist' AND r.state='active'
        WHERE e.recording_id=i.recording_id AND e.deleted=0));
DROP VIEW current_download_authority;
CREATE VIEW current_download_authority AS
SELECT i.id FROM download_intents i
JOIN retained_download_intents retained ON retained.id=i.id
JOIN users account ON account.id = i.account_id
JOIN recordings recording ON recording.id = i.recording_id AND recording.library_id = i.library_id
JOIN library_access_grants access ON access.library_id = i.library_id AND access.user_id = i.account_id AND access.id = i.access_grant_id
JOIN sources source ON source.id = i.provider_source_id AND source.recording_id = i.recording_id AND source.ready = 1
JOIN provider_sources ps ON ps.source_id = source.id AND ps.recording_id = i.recording_id
    AND ps.binding_id = i.binding_id AND ps.installation_id = i.installation_id AND ps.provider_source_id = i.remote_source_id
JOIN provider_items binding ON binding.id = i.binding_id AND binding.recording_id = i.recording_id
    AND binding.installation_id = i.installation_id AND binding.revision = i.binding_revision AND binding.retired = 0
JOIN provider_source_state source_state ON source_state.binding_id = binding.id AND source_state.generation = ps.generation
JOIN plugin_installations installation ON installation.id = i.installation_id AND installation.user_id = i.account_id
    AND installation.active = 1 AND installation.revision = i.installation_revision AND installation.service_id = i.service_id
    AND installation.audio_generation = i.audio_generation
JOIN plugin_services service ON service.id = i.service_id AND service.active = 1 AND service.revision = i.service_revision
WHERE i.state IN ('queued','running','verifying')
    AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'sources');
