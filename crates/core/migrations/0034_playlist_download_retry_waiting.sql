ALTER TABLE download_playlist_retry_work ADD COLUMN command_generation INTEGER NOT NULL DEFAULT 1 CHECK(command_generation>0);
ALTER TABLE download_playlist_retry_work ADD COLUMN scan_complete INTEGER NOT NULL DEFAULT 0 CHECK(scan_complete IN (0,1));
ALTER TABLE download_playlist_retry_work ADD COLUMN scan_next_attempt_at INTEGER NOT NULL DEFAULT 0 CHECK(scan_next_attempt_at>=0);
CREATE UNIQUE INDEX download_playlist_retry_identity ON download_playlist_retry_work(retention_id,host_device);

CREATE TABLE download_playlist_retry_waiting (
    retention_id TEXT NOT NULL,
    host_device TEXT NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    failed_intent_id TEXT NOT NULL REFERENCES download_intents(id),
    command_generation INTEGER NOT NULL CHECK(command_generation>0),
    next_attempt_at INTEGER NOT NULL CHECK(next_attempt_at>=0),
    PRIMARY KEY(retention_id,recording_id),
    FOREIGN KEY(retention_id,host_device) REFERENCES download_retentions(id,host_device),
    FOREIGN KEY(retention_id,host_device) REFERENCES download_playlist_retry_work(retention_id,host_device)
);
CREATE INDEX download_playlist_retry_waiting_due ON download_playlist_retry_waiting(retention_id,host_device,command_generation,next_attempt_at,recording_id);
CREATE INDEX download_playlist_retry_waiting_obsolete ON download_playlist_retry_waiting(retention_id,command_generation,recording_id);
CREATE INDEX download_playlist_retry_waiting_failure ON download_playlist_retry_waiting(failed_intent_id);
CREATE INDEX download_playlist_retry_waiting_recording ON download_playlist_retry_waiting(recording_id);
