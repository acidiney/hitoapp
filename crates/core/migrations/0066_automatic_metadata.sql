CREATE TABLE account_metadata_automation (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    artist_lookup INTEGER NOT NULL DEFAULT 1 CHECK(artist_lookup IN (0,1)),
    artwork INTEGER NOT NULL DEFAULT 1 CHECK(artwork IN (0,1)),
    service_search INTEGER NOT NULL DEFAULT 1 CHECK(service_search IN (0,1)),
    matching_finished_at INTEGER,
    matching_matched INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE artist_matching_jobs (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    phase TEXT NOT NULL DEFAULT 'tags' CHECK(phase IN ('tags','provider')),
    cursor TEXT CHECK(cursor IS NULL OR length(cursor) <= 128),
    rerun INTEGER NOT NULL DEFAULT 0 CHECK(rerun IN (0,1)),
    attempts INTEGER NOT NULL DEFAULT 0,
    matched INTEGER NOT NULL DEFAULT 0,
    lease_token TEXT,
    lease_until INTEGER,
    next_run_at INTEGER NOT NULL DEFAULT 0,
    requested_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX artist_matching_jobs_due ON artist_matching_jobs(next_run_at);
CREATE TABLE cover_art_lookups (
    user_id TEXT NOT NULL REFERENCES users(id),
    kind TEXT NOT NULL CHECK(kind IN ('release','release_group')),
    mbid TEXT NOT NULL CHECK(length(mbid)=36),
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
CREATE INDEX cover_art_lookups_queue ON cover_art_lookups(next_attempt_at) WHERE state IN ('queued','running');
CREATE INDEX cover_art_lookups_asset ON cover_art_lookups(user_id,content_hash) WHERE content_hash IS NOT NULL;
