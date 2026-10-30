CREATE TABLE artist_association_name_matches (
    association_id TEXT PRIMARY KEY REFERENCES recording_artist_associations(id),
    evidence_kind TEXT NOT NULL CHECK(evidence_kind='name_discography'),
    receipt_order INTEGER NOT NULL REFERENCES artist_identity_choice_receipts(acceptance_order),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TRIGGER artist_association_name_match_immutable BEFORE UPDATE ON artist_association_name_matches BEGIN
    SELECT RAISE(ABORT,'Artist association name match provenance is immutable');
END;
CREATE TRIGGER artist_association_name_match_retained BEFORE DELETE ON artist_association_name_matches BEGIN
    SELECT RAISE(ABORT,'Artist association name match provenance must be retained');
END;
CREATE TABLE artist_name_checks (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL,
    observation_id TEXT NOT NULL,
    artist_operation_id TEXT,
    PRIMARY KEY(user_id,recording_id)
);
