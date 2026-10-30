CREATE TABLE recording_artist_url_observations (
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    url TEXT NOT NULL CHECK(length(CAST(url AS BLOB)) BETWEEN 9 AND 512 AND url LIKE 'https://%'),
    PRIMARY KEY(observation_id,url)
);
CREATE INDEX recording_artist_url_observation_recording ON recording_artist_url_observations(recording_id);
CREATE TRIGGER recording_artist_url_observation_immutable BEFORE UPDATE ON recording_artist_url_observations BEGIN
    SELECT RAISE(ABORT,'Artist page observations are immutable');
END;
CREATE TRIGGER recording_artist_url_observation_retained BEFORE DELETE ON recording_artist_url_observations BEGIN
    SELECT RAISE(ABORT,'Artist page observations must be retained');
END;
