CREATE TABLE recording_musicbrainz_observations (
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id),
    entity TEXT NOT NULL CHECK(entity IN ('release','release_group','recording','release_track')),
    mbid TEXT NOT NULL CHECK(length(mbid)=36
        AND mbid GLOB '[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]'),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    release_id TEXT REFERENCES releases(id),
    PRIMARY KEY(observation_id,entity)
);
CREATE INDEX recording_musicbrainz_observation_recording ON recording_musicbrainz_observations(recording_id,entity);
CREATE TRIGGER recording_musicbrainz_observation_immutable BEFORE UPDATE ON recording_musicbrainz_observations BEGIN
    SELECT RAISE(ABORT,'MusicBrainz tag identity observations are immutable');
END;
CREATE TRIGGER recording_musicbrainz_observation_retained BEFORE DELETE ON recording_musicbrainz_observations BEGIN
    SELECT RAISE(ABORT,'MusicBrainz tag identity observations must be retained');
END;
