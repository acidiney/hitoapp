CREATE TABLE account_community_preferences (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    revision INTEGER NOT NULL CHECK(revision > 0),
    playback_reporting INTEGER NOT NULL CHECK(playback_reporting IN (0,1)),
    metadata_corrections INTEGER NOT NULL CHECK(metadata_corrections IN (0,1))
);
