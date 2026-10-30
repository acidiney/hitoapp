CREATE TABLE recording_isrc_observations (
    observation_id TEXT PRIMARY KEY REFERENCES metadata_observations(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    isrc TEXT NOT NULL CHECK(length(isrc)=12
        AND isrc GLOB '[A-Z][A-Z][A-Z0-9][A-Z0-9][A-Z0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]'),
    origin TEXT NOT NULL CHECK(origin IN ('file_tag','provider_response'))
);
CREATE INDEX recording_isrc_observation_recording ON recording_isrc_observations(recording_id);
CREATE TRIGGER recording_isrc_observation_immutable BEFORE UPDATE ON recording_isrc_observations BEGIN
    SELECT RAISE(ABORT,'ISRC observations are immutable');
END;
CREATE TRIGGER recording_isrc_observation_retained BEFORE DELETE ON recording_isrc_observations BEGIN
    SELECT RAISE(ABORT,'ISRC observations must be retained');
END;
CREATE TABLE artist_association_provider_matches (
    association_id TEXT PRIMARY KEY REFERENCES recording_artist_associations(id),
    evidence_kind TEXT NOT NULL CHECK(evidence_kind IN ('isrc','provider_url')),
    receipt_order INTEGER NOT NULL REFERENCES artist_identity_choice_receipts(acceptance_order),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TRIGGER artist_association_provider_match_immutable BEFORE UPDATE ON artist_association_provider_matches BEGIN
    SELECT RAISE(ABORT,'Artist association provider match provenance is immutable');
END;
CREATE TRIGGER artist_association_provider_match_retained BEFORE DELETE ON artist_association_provider_matches BEGIN
    SELECT RAISE(ABORT,'Artist association provider match provenance must be retained');
END;
CREATE TABLE artist_provider_checks (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL,
    observation_id TEXT NOT NULL,
    artist_operation_id TEXT,
    PRIMARY KEY(user_id,recording_id)
);
