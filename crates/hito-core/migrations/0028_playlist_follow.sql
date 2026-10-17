CREATE TEMP TABLE hito_follow_sync_high_water AS
SELECT COALESCE(MAX(seq), 0) AS sequence FROM sqlite_sequence WHERE name = 'sync_events';
CREATE TEMP TABLE hito_follow_import_receipts AS SELECT * FROM playlist_import_receipts;
DROP TABLE playlist_import_receipts;
CREATE TABLE sync_events_with_follows (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL REFERENCES users(id),
    origin TEXT NOT NULL CHECK(origin IN ('client_operation','playlist_import','playlist_follow')),
    device_id TEXT,
    event_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    UNIQUE(user_id,origin,event_id),
    CHECK((origin = 'client_operation' AND device_id IS NOT NULL)
        OR (origin IN ('playlist_import','playlist_follow') AND device_id IS NULL))
);
INSERT INTO sync_events_with_follows SELECT * FROM sync_events;
DROP TABLE sync_events;
ALTER TABLE sync_events_with_follows RENAME TO sync_events;
INSERT INTO sqlite_sequence(name,seq)
SELECT 'sync_events',sequence FROM hito_follow_sync_high_water
WHERE NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name = 'sync_events');
UPDATE sqlite_sequence SET seq = MAX(seq,(SELECT sequence FROM hito_follow_sync_high_water))
WHERE name = 'sync_events';
CREATE INDEX sync_events_account_sequence ON sync_events(user_id,sequence);
CREATE TABLE playlist_import_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    playlist_id TEXT NOT NULL REFERENCES playlists(id),
    publication_sequence INTEGER NOT NULL REFERENCES sync_events(sequence),
    PRIMARY KEY(user_id,request_id)
);
INSERT INTO playlist_import_receipts SELECT * FROM hito_follow_import_receipts;
DROP TABLE hito_follow_import_receipts;
DROP TABLE hito_follow_sync_high_water;

CREATE TABLE playlist_follows (
    playlist_id TEXT PRIMARY KEY REFERENCES playlists(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id),
    installation_revision INTEGER NOT NULL CHECK(installation_revision > 0),
    service_id TEXT NOT NULL REFERENCES plugin_services(id),
    service_revision INTEGER NOT NULL CHECK(service_revision > 0),
    provider TEXT NOT NULL,
    provider_playlist_id TEXT NOT NULL,
    snapshot TEXT NOT NULL,
    attribution TEXT NOT NULL,
    interval_seconds INTEGER NOT NULL CHECK(interval_seconds IN (3600,86400)),
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation > 0),
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_refreshed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    next_refresh_at INTEGER NOT NULL,
    error TEXT
);
CREATE INDEX playlist_follow_due ON playlist_follows(active,next_refresh_at,playlist_id);
CREATE INDEX playlist_follow_account ON playlist_follows(user_id,active,playlist_id);
CREATE TABLE playlist_follow_entries (
    playlist_id TEXT NOT NULL REFERENCES playlist_follows(playlist_id),
    entry_id TEXT NOT NULL,
    occurrence_id TEXT,
    provider_recording_id TEXT,
    kind TEXT NOT NULL CHECK(kind IN ('recording','unavailable','unknown')),
    title TEXT,
    artist TEXT,
    CHECK((kind = 'recording' AND provider_recording_id IS NOT NULL AND title IS NULL AND artist IS NULL)
        OR kind = 'unavailable'
        OR (kind = 'unknown' AND provider_recording_id IS NULL AND title IS NULL AND artist IS NULL)),
    PRIMARY KEY(playlist_id,entry_id),
    FOREIGN KEY(playlist_id,entry_id) REFERENCES playlist_entries(playlist_id,id)
);
CREATE TABLE playlist_follow_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    playlist_id TEXT NOT NULL REFERENCES playlist_follows(playlist_id),
    publication_sequence INTEGER NOT NULL REFERENCES sync_events(sequence),
    PRIMARY KEY(user_id,request_id)
);
CREATE TABLE playlist_follow_jobs (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    playlist_id TEXT NOT NULL REFERENCES playlist_follows(playlist_id),
    generation INTEGER NOT NULL CHECK(generation > 0),
    status TEXT NOT NULL CHECK(status IN ('queued','running','complete','failed','cancelled')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
    next_attempt_at INTEGER NOT NULL DEFAULT (unixepoch()),
    lease_token TEXT,
    lease_until INTEGER,
    error TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK((status = 'running') = (lease_token IS NOT NULL))
);
CREATE UNIQUE INDEX playlist_follow_pending ON playlist_follow_jobs(playlist_id) WHERE status IN ('queued','running');
CREATE INDEX playlist_follow_queue ON playlist_follow_jobs(status,next_attempt_at,created_at,id);
CREATE INDEX playlist_follow_expired ON playlist_follow_jobs(lease_until) WHERE lease_token IS NOT NULL;
CREATE INDEX playlist_follow_job_account ON playlist_follow_jobs(user_id,status,created_at,id);
CREATE TABLE playlist_follow_refresh_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    playlist_id TEXT NOT NULL REFERENCES playlist_follows(playlist_id),
    job_id TEXT NOT NULL REFERENCES playlist_follow_jobs(id),
    PRIMARY KEY(user_id,request_id)
);
CREATE VIEW current_playlist_follow_authority AS
SELECT f.playlist_id FROM playlist_follows f
JOIN users account ON account.id = f.user_id
JOIN playlists p ON p.id = f.playlist_id AND p.user_id = f.user_id AND p.deleted = 0 AND p.kind = 'followed'
JOIN plugin_installations installation ON installation.id = f.installation_id
    AND installation.user_id = f.user_id AND installation.active = 1
    AND installation.revision = f.installation_revision AND installation.service_id = f.service_id
    AND json_extract(installation.manifest,'$.id') = f.provider
JOIN plugin_services service ON service.id = f.service_id AND service.active = 1 AND service.revision = f.service_revision
WHERE f.active = 1 AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'playlists');
