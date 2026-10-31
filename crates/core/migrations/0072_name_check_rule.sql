-- Name checks record the rule that settled them. Checks of an earlier rule no longer hide a
-- recording, so every account that has any walks again under the corroboration by title rule.
ALTER TABLE artist_name_checks ADD COLUMN rule INTEGER NOT NULL DEFAULT 1;

-- A walk counts as having met an unavailable provider once, however many batches saw it, so that
-- attempts count walks and not batches.
ALTER TABLE artist_matching_jobs ADD COLUMN unavailable INTEGER NOT NULL DEFAULT 0 CHECK(unavailable IN (0,1));

INSERT INTO artist_matching_jobs(user_id)
    SELECT DISTINCT user_id FROM artist_name_checks WHERE true
    ON CONFLICT(user_id) DO UPDATE SET rerun=1;
