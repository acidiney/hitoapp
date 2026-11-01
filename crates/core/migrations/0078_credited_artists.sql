-- Structured artist credits a provider stated for one observation. Both tables are written once
-- beside the observation and never change; older observations simply have no rows.
CREATE TABLE recording_credit_observations (
    observation_id TEXT PRIMARY KEY REFERENCES metadata_observations(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    credit_label TEXT NOT NULL CHECK(length(CAST(credit_label AS BLOB)) BETWEEN 1 AND 4352)
);
CREATE INDEX recording_credit_observation_recording ON recording_credit_observations(recording_id);
CREATE TRIGGER recording_credit_observation_immutable BEFORE UPDATE ON recording_credit_observations BEGIN
    SELECT RAISE(ABORT,'Credit observations are immutable');
END;

CREATE TABLE recording_credited_artist_observations (
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id),
    position INTEGER NOT NULL CHECK(position BETWEEN 0 AND 15),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    name TEXT NOT NULL CHECK(length(CAST(name AS BLOB)) BETWEEN 1 AND 256),
    role TEXT NOT NULL CHECK(role IN ('main','featured')),
    join_phrase TEXT CHECK(join_phrase IS NULL OR length(CAST(join_phrase AS BLOB)) BETWEEN 1 AND 32),
    provider_artist_id TEXT CHECK(provider_artist_id IS NULL OR length(CAST(provider_artist_id AS BLOB)) BETWEEN 1 AND 128),
    page_url TEXT CHECK(page_url IS NULL OR (length(CAST(page_url AS BLOB)) BETWEEN 9 AND 512 AND page_url LIKE 'https://%')),
    PRIMARY KEY(observation_id,position)
);
CREATE INDEX recording_credited_artist_observation_recording ON recording_credited_artist_observations(recording_id);
CREATE TRIGGER recording_credited_artist_observation_immutable BEFORE UPDATE ON recording_credited_artist_observations BEGIN
    SELECT RAISE(ABORT,'Credited artist observations are immutable');
END;
