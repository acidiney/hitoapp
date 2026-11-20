CREATE TABLE artist_association_member_matches (
    association_id TEXT PRIMARY KEY REFERENCES recording_artist_associations(id),
    evidence_kind TEXT NOT NULL CHECK(evidence_kind IN ('solo_credit','listed_artist_id','file_tag_artist_id')),
    receipt_order INTEGER NOT NULL REFERENCES artist_identity_choice_receipts(acceptance_order),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TRIGGER artist_association_member_match_immutable BEFORE UPDATE ON artist_association_member_matches BEGIN
    SELECT RAISE(ABORT,'Artist association member match provenance is immutable');
END;
CREATE TRIGGER artist_association_member_match_retained BEFORE DELETE ON artist_association_member_matches
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT a.user_id FROM recording_artist_associations a WHERE a.id=old.association_id))
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=(SELECT a.recording_id FROM recording_artist_associations a WHERE a.id=old.association_id))) BEGIN
    SELECT RAISE(ABORT,'Artist association member match provenance must be retained');
END;

CREATE TABLE artist_name_misses (
    user_id TEXT NOT NULL,
    artist_id TEXT NOT NULL,
    PRIMARY KEY(user_id,artist_id),
    FOREIGN KEY(user_id,artist_id) REFERENCES account_artists(user_id,id)
);

CREATE TABLE account_credit_splits (
    user_id TEXT NOT NULL,
    artist_id TEXT NOT NULL,
    position INTEGER NOT NULL CHECK(typeof(position)='integer' AND position BETWEEN 0 AND 15),
    name TEXT NOT NULL COLLATE BINARY CHECK(length(name) BETWEEN 1 AND 256),
    join_phrase TEXT CHECK(join_phrase IS NULL OR length(join_phrase) BETWEEN 1 AND 64),
    PRIMARY KEY(user_id,artist_id,position),
    FOREIGN KEY(user_id,artist_id) REFERENCES account_artists(user_id,id)
);
