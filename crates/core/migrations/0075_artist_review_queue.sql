-- The artists the name walk found under a credit's name and could not tell apart, kept so the
-- account can decide in one step without another MusicBrainz request. One row per credit and
-- artist; a later walk replaces the credit's rows.
CREATE TABLE artist_name_candidates (
    user_id TEXT NOT NULL REFERENCES users(id),
    artist_id TEXT NOT NULL,
    provider_artist_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    kind TEXT,
    disambiguation TEXT,
    country TEXT,
    corroborated_titles INTEGER NOT NULL,
    PRIMARY KEY(user_id,artist_id,provider_artist_id)
);
-- "Not this artist" and "None of these": a dismissed candidate is not offered again for the credit.
CREATE TABLE artist_review_dismissals (
    user_id TEXT NOT NULL REFERENCES users(id),
    artist_id TEXT NOT NULL,
    provider_artist_id TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,artist_id,provider_artist_id)
);
-- Provenance of an association the account created by confirming a candidate.
CREATE TABLE artist_association_candidate_confirmations (
    association_id TEXT PRIMARY KEY REFERENCES recording_artist_associations(id),
    evidence_kind TEXT NOT NULL CHECK(evidence_kind='user_confirmed_candidate'),
    receipt_order INTEGER NOT NULL REFERENCES artist_identity_choice_receipts(acceptance_order),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TRIGGER artist_association_candidate_confirmation_immutable BEFORE UPDATE ON artist_association_candidate_confirmations BEGIN
    SELECT RAISE(ABORT,'Artist association candidate confirmation provenance is immutable');
END;
CREATE TRIGGER artist_association_candidate_confirmation_retained BEFORE DELETE ON artist_association_candidate_confirmations BEGIN
    SELECT RAISE(ABORT,'Artist association candidate confirmation provenance must be retained');
END;

-- Rule 5 stores the candidates of every uncertain credit, so name checks of the earlier rules are
-- examined again.
INSERT INTO artist_matching_jobs(user_id)
    SELECT DISTINCT user_id FROM artist_name_checks WHERE true
    ON CONFLICT(user_id) DO UPDATE SET rerun=1;
