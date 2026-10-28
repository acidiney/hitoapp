CREATE TABLE artist_association_matches (
    association_id TEXT PRIMARY KEY REFERENCES recording_artist_associations(id),
    evidence_kind TEXT NOT NULL CHECK(evidence_kind IN ('file_tag_artist_id')),
    receipt_order INTEGER NOT NULL REFERENCES artist_identity_choice_receipts(acceptance_order),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TRIGGER artist_association_match_immutable BEFORE UPDATE ON artist_association_matches BEGIN
    SELECT RAISE(ABORT,'Artist association match provenance is immutable');
END;
CREATE TRIGGER artist_association_match_retained BEFORE DELETE ON artist_association_matches BEGIN
    SELECT RAISE(ABORT,'Artist association match provenance must be retained');
END;
CREATE TABLE artist_match_dismissals (
    user_id TEXT NOT NULL REFERENCES users(id),
    artist_id TEXT NOT NULL,
    group_id TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,artist_id,group_id),
    FOREIGN KEY(user_id,group_id) REFERENCES account_artist_identity_groups(user_id,id)
);
CREATE TABLE artist_match_checks (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL,
    observation_id TEXT NOT NULL,
    artist_operation_id TEXT,
    PRIMARY KEY(user_id,recording_id)
);
