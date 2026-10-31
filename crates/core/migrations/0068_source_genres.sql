CREATE TABLE source_genres (
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    genre_key TEXT NOT NULL CHECK(length(genre_key) BETWEEN 1 AND 64),
    label TEXT NOT NULL CHECK(length(label) BETWEEN 1 AND 64),
    PRIMARY KEY(source_id, genre_key)
);
CREATE INDEX source_genres_by_genre ON source_genres(genre_key, source_id, label);
