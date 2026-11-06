CREATE TABLE account_source_preferences (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK(revision > 0),
    fallback INTEGER NOT NULL DEFAULT 1 CHECK(fallback IN (0,1)),
    entries TEXT NOT NULL CHECK(json_valid(entries) AND json_type(entries) = 'array' AND json_array_length(entries) <= 32)
);
