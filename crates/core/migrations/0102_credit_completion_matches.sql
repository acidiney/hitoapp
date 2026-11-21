-- A credit whose current choices made by the account name exactly one artist gives that artist to
-- the credit's remaining undecided tracks. The provenance keeps how many of the account's own
-- decisions supplied it.
CREATE TABLE artist_association_completion_matches (
    association_id TEXT PRIMARY KEY REFERENCES recording_artist_associations(id),
    source_tracks INTEGER NOT NULL CHECK(source_tracks>=1),
    receipt_order INTEGER NOT NULL REFERENCES artist_identity_choice_receipts(acceptance_order),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TRIGGER artist_association_completion_match_immutable BEFORE UPDATE ON artist_association_completion_matches BEGIN
    SELECT RAISE(ABORT,'Artist association completion match provenance is immutable');
END;
CREATE TRIGGER artist_association_completion_match_retained BEFORE DELETE ON artist_association_completion_matches
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT a.user_id FROM recording_artist_associations a WHERE a.id=old.association_id))
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=(SELECT a.recording_id FROM recording_artist_associations a WHERE a.id=old.association_id))) BEGIN
    SELECT RAISE(ABORT,'Artist association completion match provenance must be retained');
END;

-- Accounts that already made choices walk again so their earlier decisions complete their credits.
INSERT INTO artist_matching_jobs(user_id)
    SELECT DISTINCT user_id FROM recording_artist_identity_choices WHERE true
    ON CONFLICT(user_id) DO UPDATE SET rerun=1;
