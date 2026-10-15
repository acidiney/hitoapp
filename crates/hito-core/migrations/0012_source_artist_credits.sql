CREATE TABLE source_artist_credits (
    source_id TEXT NOT NULL PRIMARY KEY REFERENCES source_metadata(source_id) ON DELETE CASCADE,
    artist_id TEXT NOT NULL REFERENCES artists(id)
);
INSERT INTO source_artist_credits(source_id, artist_id)
SELECT m.source_id, c.id FROM source_metadata m
JOIN sources s ON s.id = m.source_id JOIN recordings r ON r.id = s.recording_id
JOIN artists c ON c.library_id = r.library_id AND c.name = m.artist COLLATE BINARY;
CREATE INDEX source_artist_lookup ON source_artist_credits(artist_id, source_id);
DROP VIEW artist_releases;
CREATE VIEW artist_releases AS
    SELECT c.id AS artist_id, a.id AS release_id FROM artists c
    JOIN releases a ON a.library_id = c.library_id AND a.artist = c.name COLLATE BINARY
    WHERE EXISTS (SELECT 1 FROM source_metadata m WHERE m.release_id = a.id)
    UNION ALL
    SELECT c.id, a.id FROM artists c
    CROSS JOIN source_artist_credits credit ON credit.artist_id = c.id
    CROSS JOIN source_metadata m ON m.source_id = credit.source_id
    CROSS JOIN releases a ON a.id = m.release_id AND a.library_id = c.library_id;
