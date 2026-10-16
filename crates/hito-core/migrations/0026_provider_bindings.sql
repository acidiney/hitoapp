PRAGMA defer_foreign_keys = ON;
DROP TRIGGER provider_source_state_insert;
DROP TRIGGER provider_item_owner;
DROP TRIGGER provider_item_immutable;
DROP TRIGGER provider_recording_library;

CREATE TABLE provider_items_next (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id),
    provider_recording_id TEXT NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    role TEXT NOT NULL CHECK(role IN ('primary','source')),
    evidence TEXT NOT NULL CHECK(evidence IN ('provider_identity','user_confirmed')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    retired INTEGER NOT NULL DEFAULT 0 CHECK(retired IN (0,1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(id,installation_id,recording_id)
);
INSERT INTO provider_items_next(id,installation_id,provider_recording_id,recording_id,role,evidence)
SELECT lower(hex(randomblob(16))),installation_id,provider_recording_id,recording_id,'primary','provider_identity'
FROM provider_items;
CREATE INDEX provider_items_recording ON provider_items_next(recording_id,retired,id);
ALTER TABLE recordings ADD COLUMN metadata_binding_id TEXT REFERENCES provider_items_next(id);
UPDATE recordings SET metadata_binding_id = (SELECT id FROM provider_items_next item WHERE item.recording_id = recordings.id)
WHERE EXISTS(SELECT 1 FROM provider_items_next item WHERE item.recording_id = recordings.id);

CREATE TEMP TABLE hito_binding_observation_sequence AS SELECT seq FROM sqlite_sequence WHERE name = 'metadata_observations';
CREATE TABLE metadata_observations_next (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    source_id TEXT REFERENCES sources(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    release_id TEXT REFERENCES releases(id),
    provider TEXT NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('legacy_normalized','file_inspection','provider_response')),
    observed_at INTEGER,
    file_checksum TEXT,
    inspection_version INTEGER,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    album TEXT,
    album_artist TEXT,
    duration_ms INTEGER NOT NULL,
    disc_number INTEGER,
    track_number INTEGER,
    tags TEXT,
    plugin_installation_id TEXT REFERENCES plugin_installations(id),
    provider_recording_id TEXT,
    provider_revision TEXT,
    provider_metadata TEXT,
    provider_digest TEXT,
    provider_binding_id TEXT REFERENCES provider_items_next(id),
    UNIQUE(provider_binding_id,provider_digest),
    UNIQUE(source_id,file_checksum,inspection_version)
);
INSERT INTO metadata_observations_next
SELECT o.sequence,o.id,o.source_id,o.recording_id,o.release_id,o.provider,o.origin,o.observed_at,o.file_checksum,
    o.inspection_version,o.title,o.artist,o.album,o.album_artist,o.duration_ms,o.disc_number,o.track_number,o.tags,
    o.plugin_installation_id,o.provider_recording_id,o.provider_revision,o.provider_metadata,o.provider_digest,item.id
FROM metadata_observations o LEFT JOIN provider_items_next item
    ON item.installation_id = o.plugin_installation_id AND item.provider_recording_id = o.provider_recording_id
    AND item.recording_id = o.recording_id;
CREATE INDEX provider_source_state_observation ON provider_source_state(observation_id);
DROP TABLE metadata_observations;
ALTER TABLE metadata_observations_next RENAME TO metadata_observations;
UPDATE sqlite_sequence SET seq = max(seq, COALESCE((SELECT seq FROM hito_binding_observation_sequence),0))
WHERE name = 'metadata_observations';
DROP TABLE hito_binding_observation_sequence;
CREATE INDEX observations_recording ON metadata_observations(recording_id,sequence DESC);
CREATE INDEX observations_release ON metadata_observations(release_id,sequence DESC);

CREATE TABLE provider_source_state_next (
    binding_id TEXT PRIMARY KEY REFERENCES provider_items_next(id),
    generation INTEGER NOT NULL DEFAULT 0 CHECK(generation >= 0),
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id)
);
INSERT INTO provider_source_state_next(binding_id,generation,observation_id)
SELECT item.id,state.generation,state.observation_id FROM provider_source_state state
JOIN provider_items_next item ON item.recording_id = state.recording_id
WHERE state.observation_id IS NOT NULL;
CREATE TABLE provider_sources_next (
    source_id TEXT PRIMARY KEY,
    binding_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    recording_id TEXT NOT NULL,
    provider_source_id TEXT NOT NULL,
    label TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation >= 0),
    UNIQUE(binding_id,provider_source_id),
    FOREIGN KEY(binding_id,installation_id,recording_id) REFERENCES provider_items_next(id,installation_id,recording_id),
    FOREIGN KEY(source_id,recording_id) REFERENCES sources(id,recording_id)
);
INSERT INTO provider_sources_next(source_id,binding_id,installation_id,recording_id,provider_source_id,label,generation)
SELECT ps.source_id,item.id,ps.installation_id,ps.recording_id,ps.provider_source_id,ps.label,ps.generation
FROM provider_sources ps JOIN provider_items_next item
ON item.installation_id = ps.installation_id AND item.recording_id = ps.recording_id;
DROP TABLE provider_source_state;
DROP TABLE provider_sources;
DROP TABLE provider_items;
ALTER TABLE provider_items_next RENAME TO provider_items;
ALTER TABLE provider_source_state_next RENAME TO provider_source_state;
ALTER TABLE provider_sources_next RENAME TO provider_sources;
CREATE UNIQUE INDEX provider_items_current_identity ON provider_items(installation_id,provider_recording_id) WHERE retired = 0;
CREATE INDEX provider_sources_generation ON provider_sources(binding_id,generation,source_id);
CREATE INDEX provider_sources_recording ON provider_sources(installation_id,provider_source_id,binding_id);

CREATE TRIGGER provider_item_owner BEFORE INSERT ON provider_items
WHEN NOT EXISTS(SELECT 1 FROM plugin_installations p JOIN libraries l ON l.owner_id = p.user_id
    JOIN recordings r ON r.library_id = l.id WHERE p.id = new.installation_id AND r.id = new.recording_id) BEGIN
    SELECT RAISE(ABORT, 'Provider recordings require their account library');
END;
CREATE TRIGGER provider_item_immutable BEFORE UPDATE ON provider_items
WHEN old.id IS NOT new.id OR old.installation_id IS NOT new.installation_id
    OR old.provider_recording_id IS NOT new.provider_recording_id OR old.recording_id IS NOT new.recording_id
    OR old.role IS NOT new.role OR old.evidence IS NOT new.evidence OR old.created_at IS NOT new.created_at
    OR old.retired != 0 OR new.retired != 1 OR new.revision != old.revision + 1 BEGIN
    SELECT RAISE(ABORT, 'Provider binding identity is immutable');
END;
CREATE TRIGGER provider_item_retained BEFORE DELETE ON provider_items BEGIN
    SELECT RAISE(ABORT, 'Provider binding history must be retained');
END;
CREATE TRIGGER provider_recording_library BEFORE UPDATE OF library_id ON recordings
WHEN old.library_id != new.library_id AND EXISTS(SELECT 1 FROM provider_items WHERE recording_id = old.id) BEGIN
    SELECT RAISE(ABORT, 'Provider recording ownership is immutable');
END;
CREATE TRIGGER provider_metadata_owner BEFORE UPDATE OF metadata_binding_id ON recordings
WHEN new.metadata_binding_id IS NOT NULL AND NOT EXISTS(
    SELECT 1 FROM provider_items item WHERE item.id = new.metadata_binding_id AND item.recording_id = new.id AND item.role = 'primary'
) BEGIN
    SELECT RAISE(ABORT, 'Provider metadata requires its primary binding');
END;
CREATE TRIGGER provider_source_owner BEFORE INSERT ON provider_sources
WHEN EXISTS(SELECT 1 FROM provider_sources ps JOIN provider_items item ON item.id = ps.binding_id AND item.retired = 0
    WHERE ps.installation_id = new.installation_id AND ps.provider_source_id = new.provider_source_id AND ps.binding_id != new.binding_id) BEGIN
    SELECT RAISE(ABORT, 'Provider source identity is already bound');
END;

CREATE TEMP TABLE hito_binding_fk_guard (value INTEGER NOT NULL CHECK(value = 0));
INSERT INTO hito_binding_fk_guard SELECT 1 FROM pragma_foreign_key_check;
DROP TABLE hito_binding_fk_guard;
PRAGMA defer_foreign_keys = OFF;
