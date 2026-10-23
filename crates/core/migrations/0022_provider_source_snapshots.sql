CREATE TABLE provider_source_state (
    recording_id TEXT PRIMARY KEY REFERENCES provider_items(recording_id),
    generation INTEGER NOT NULL DEFAULT 0 CHECK(generation >= 0),
    observation_id TEXT REFERENCES metadata_observations(id)
);
INSERT INTO provider_source_state(recording_id, generation, observation_id)
SELECT item.recording_id, 1, r.metadata_observation_id FROM provider_items item
JOIN recordings r ON r.id = item.recording_id;
CREATE TRIGGER provider_source_state_insert AFTER INSERT ON provider_items BEGIN
    INSERT INTO provider_source_state(recording_id) VALUES(new.recording_id);
END;

ALTER TABLE provider_sources ADD COLUMN generation INTEGER NOT NULL DEFAULT 0 CHECK(generation >= 0);
UPDATE provider_sources SET generation = 1
WHERE source_id IN (SELECT id FROM sources WHERE ready = 1);
DROP INDEX provider_sources_recording;
CREATE INDEX provider_sources_recording ON provider_sources(installation_id, recording_id, generation, source_id);
CREATE INDEX provider_sources_generation ON provider_sources(recording_id, generation, source_id);
CREATE INDEX nonplugin_sources_recording ON sources(recording_id, id) WHERE provider != 'plugin';
