CREATE TABLE account_community_ranking_preferences (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    revision INTEGER NOT NULL CHECK(revision > 0),
    use_source_rankings INTEGER NOT NULL CHECK(use_source_rankings IN (0,1))
);
