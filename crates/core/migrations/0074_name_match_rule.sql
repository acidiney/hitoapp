-- Name matches record the rule that made them. Matches made before rule 3 (version titles,
-- continuation and coverage) are rule 2 and no longer count toward an artist's coverage of a
-- credit; matches made after migration 0073 was installed are rule 3.
DROP TRIGGER artist_association_name_match_immutable;
ALTER TABLE artist_association_name_matches ADD COLUMN rule INTEGER NOT NULL DEFAULT 2;
UPDATE artist_association_name_matches SET rule=3
WHERE created_at >= COALESCE((SELECT CAST(strftime('%s',installed_on) AS INTEGER) FROM _sqlx_migrations WHERE version=73), 9223372036854775807);
CREATE TRIGGER artist_association_name_match_immutable BEFORE UPDATE ON artist_association_name_matches BEGIN
    SELECT RAISE(ABORT,'Artist association name match provenance is immutable');
END;

-- Rule 4 (distinct titles for the share, the tail asked of every namesake, personal choices as a
-- veto) examines name checks of the earlier rules again.
INSERT INTO artist_matching_jobs(user_id)
    SELECT DISTINCT user_id FROM artist_name_checks WHERE true
    ON CONFLICT(user_id) DO UPDATE SET rerun=1;
