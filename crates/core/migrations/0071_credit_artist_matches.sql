CREATE TABLE artist_association_credit_matches (
    association_id TEXT PRIMARY KEY REFERENCES recording_artist_associations(id),
    evidence_kind TEXT NOT NULL CHECK(evidence_kind IN ('file_tag_artist_id','isrc','provider_url','name_discography')),
    source_tracks INTEGER NOT NULL CHECK(source_tracks>=1),
    receipt_order INTEGER NOT NULL REFERENCES artist_identity_choice_receipts(acceptance_order),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TRIGGER artist_association_credit_match_immutable BEFORE UPDATE ON artist_association_credit_matches BEGIN
    SELECT RAISE(ABORT,'Artist association credit match provenance is immutable');
END;
CREATE TRIGGER artist_association_credit_match_retained BEFORE DELETE ON artist_association_credit_matches
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT a.user_id FROM recording_artist_associations a WHERE a.id=old.association_id))
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=(SELECT a.recording_id FROM recording_artist_associations a WHERE a.id=old.association_id))) BEGIN
    SELECT RAISE(ABORT,'Artist association credit match provenance must be retained');
END;

-- Name checks decided while a search with more results than one page counted as uncertain are
-- examined again under the score rule, and the affected accounts walk once more.
INSERT INTO artist_matching_jobs(user_id)
    SELECT DISTINCT user_id FROM artist_name_checks WHERE true
    ON CONFLICT(user_id) DO UPDATE SET rerun=1;
DELETE FROM artist_name_checks;
