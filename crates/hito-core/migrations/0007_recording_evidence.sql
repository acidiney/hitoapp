ALTER TABLE recordings RENAME COLUMN content_hash TO initial_file_checksum;
ALTER TABLE local_files ADD COLUMN file_checksum TEXT;
ALTER TABLE local_files ADD COLUMN last_scan_job TEXT;
ALTER TABLE local_files ADD COLUMN inspection_scheme TEXT;
ALTER TABLE local_files ADD COLUMN audio_digest TEXT;

UPDATE local_files SET file_checksum = (
    SELECT r.initial_file_checksum FROM recordings r
    JOIN sources s ON s.recording_id = r.id WHERE s.id = local_files.source_id
);

CREATE TABLE recording_evidence (
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    algorithm TEXT NOT NULL,
    digest TEXT NOT NULL,
    PRIMARY KEY(recording_id, algorithm, digest)
);
CREATE INDEX recording_evidence_lookup ON recording_evidence(algorithm, digest, recording_id);
CREATE INDEX local_files_checksum ON local_files(library_id, file_checksum, source_id);
CREATE INDEX local_files_scan ON local_files(library_id, scan_root, last_scan_job);
CREATE INDEX local_files_reconciliation ON local_files(library_id, source_id);
CREATE INDEX sources_metadata_owner ON sources(recording_id, ready, id);

CREATE TABLE source_metadata (
    source_id TEXT PRIMARY KEY REFERENCES sources(id),
    release_id TEXT REFERENCES releases(id),
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    duration_ms INTEGER NOT NULL CHECK(duration_ms >= 0),
    provenance TEXT NOT NULL DEFAULT 'file_tags'
);
INSERT INTO source_metadata(source_id, release_id, title, artist, duration_ms)
SELECT s.id, r.release_id, r.title, r.artist, r.duration_ms
FROM sources s JOIN recordings r ON r.id = s.recording_id WHERE s.provider = 'local';

ALTER TABLE recordings ADD COLUMN metadata_source_id TEXT REFERENCES sources(id);
UPDATE recordings SET metadata_source_id = (
    SELECT min(m.source_id) FROM source_metadata m JOIN sources s ON s.id = m.source_id
    WHERE s.recording_id = recordings.id
);
