CREATE TABLE account_automatic_copies (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    keep_played_tracks INTEGER NOT NULL DEFAULT 1 CHECK(keep_played_tracks IN (0,1)),
    budget_bytes INTEGER NOT NULL DEFAULT 5368709120 CHECK(budget_bytes BETWEEN 0 AND 1099511627776)
);

CREATE TABLE automatic_copies (
    account_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    host_device TEXT NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    access_grant_id TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(account_id,recording_id,host_device)
);

CREATE TABLE upcoming_recordings (
    session_hash TEXT NOT NULL REFERENCES sessions(token_hash) ON DELETE CASCADE,
    client TEXT NOT NULL CHECK(length(client) <= 64),
    account_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position IN (0,1)),
    announced_at INTEGER NOT NULL DEFAULT (unixepoch()),
    next_attempt_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(session_hash,client,recording_id)
);
CREATE INDEX upcoming_recordings_due ON upcoming_recordings(next_attempt_at,announced_at);
CREATE INDEX upcoming_recordings_account ON upcoming_recordings(account_id,recording_id);

ALTER TABLE listening_history ADD COLUMN play_count INTEGER NOT NULL DEFAULT 0 CHECK(play_count >= 0);
ALTER TABLE download_jobs ADD COLUMN busy_retries INTEGER NOT NULL DEFAULT 0 CHECK(busy_retries >= 0);

DROP VIEW retained_download_intents;

CREATE VIEW chosen_download_intents AS
SELECT i.id FROM download_intents i WHERE i.state!='removed' AND (
    EXISTS(SELECT 1 FROM download_retentions r WHERE r.kind='recording' AND r.state='active'
        AND r.account_id=i.account_id AND r.host_device=i.host_device AND r.recording_id=i.recording_id
        AND r.access_grant_id=i.access_grant_id)
    OR EXISTS(SELECT 1 FROM playlist_entries e JOIN playlists p ON p.id=e.playlist_id AND p.deleted=0 AND p.user_id=i.account_id
        JOIN download_retentions r ON r.playlist_id=p.id AND r.account_id=p.user_id AND r.host_device=i.host_device
            AND r.kind='playlist' AND r.state='active'
        WHERE e.recording_id=i.recording_id AND e.deleted=0));

CREATE VIEW retained_download_intents AS
SELECT i.id FROM download_intents i WHERE i.state!='removed' AND (
    EXISTS(SELECT 1 FROM download_retentions r WHERE r.kind='recording' AND r.state='active'
        AND r.account_id=i.account_id AND r.host_device=i.host_device AND r.recording_id=i.recording_id
        AND r.access_grant_id=i.access_grant_id)
    OR EXISTS(SELECT 1 FROM playlist_entries e JOIN playlists p ON p.id=e.playlist_id AND p.deleted=0 AND p.user_id=i.account_id
        JOIN download_retentions r ON r.playlist_id=p.id AND r.account_id=p.user_id AND r.host_device=i.host_device
            AND r.kind='playlist' AND r.state='active'
        WHERE e.recording_id=i.recording_id AND e.deleted=0)
    OR EXISTS(SELECT 1 FROM automatic_copies copy WHERE copy.account_id=i.account_id AND copy.host_device=i.host_device
        AND copy.recording_id=i.recording_id AND copy.access_grant_id=i.access_grant_id));
