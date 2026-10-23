CREATE TABLE personal_release_metadata (
    user_id TEXT NOT NULL REFERENCES users(id),
    release_id TEXT NOT NULL REFERENCES releases(id),
    override_title TEXT,
    override_artist TEXT,
    title_operation_id TEXT,
    artist_operation_id TEXT,
    PRIMARY KEY(user_id, release_id),
    CHECK(title_operation_id IS NOT NULL OR artist_operation_id IS NOT NULL),
    CHECK(override_title IS NULL OR title_operation_id IS NOT NULL),
    CHECK(override_artist IS NULL OR artist_operation_id IS NOT NULL)
);
CREATE INDEX observations_release ON metadata_observations(release_id, sequence DESC);
