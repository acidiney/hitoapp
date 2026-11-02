CREATE TABLE cover_art_lookups_next (
    user_id TEXT NOT NULL REFERENCES users(id),
    kind TEXT NOT NULL CHECK(kind IN ('release','release_group','artist','credited_artist')),
    mbid TEXT NOT NULL CHECK((kind = 'credited_artist' AND length(mbid) BETWEEN 1 AND 64) OR (kind <> 'credited_artist' AND length(mbid) = 36)),
    state TEXT NOT NULL CHECK(state IN ('queued','running','ready','missing','unavailable')),
    content_hash TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL DEFAULT 0,
    lease_until INTEGER,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,kind,mbid),
    CHECK((state = 'ready') = (content_hash IS NOT NULL)),
    FOREIGN KEY(user_id,content_hash) REFERENCES artwork_assets(user_id,content_hash)
);
INSERT INTO cover_art_lookups_next(user_id,kind,mbid,state,content_hash,attempts,next_attempt_at,lease_until,updated_at)
    SELECT user_id,kind,mbid,state,content_hash,attempts,next_attempt_at,lease_until,updated_at FROM cover_art_lookups;
DROP TABLE cover_art_lookups;
ALTER TABLE cover_art_lookups_next RENAME TO cover_art_lookups;
CREATE INDEX cover_art_lookups_queue ON cover_art_lookups(next_attempt_at) WHERE state IN ('queued','running');
CREATE INDEX cover_art_lookups_asset ON cover_art_lookups(user_id,content_hash) WHERE content_hash IS NOT NULL;
