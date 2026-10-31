CREATE TABLE source_unavailability (
    source_id TEXT PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
    since INTEGER NOT NULL
);
CREATE INDEX source_unavailability_by_since ON source_unavailability(since, source_id);
INSERT INTO source_unavailability(source_id, since)
    SELECT id, unixepoch() FROM sources WHERE ready = 0;
CREATE TRIGGER source_unavailable_on_insert AFTER INSERT ON sources WHEN new.ready = 0 BEGIN
    INSERT OR IGNORE INTO source_unavailability(source_id, since) VALUES (new.id, unixepoch());
END;
CREATE TRIGGER source_unavailable_on_withdrawal AFTER UPDATE OF ready ON sources WHEN old.ready = 1 AND new.ready = 0 BEGIN
    INSERT OR REPLACE INTO source_unavailability(source_id, since) VALUES (new.id, unixepoch());
END;
CREATE TRIGGER source_available_again AFTER UPDATE OF ready ON sources WHEN new.ready = 1 BEGIN
    DELETE FROM source_unavailability WHERE source_id = new.id;
END;
