ALTER TABLE source_metadata ADD COLUMN disc_number INTEGER CHECK(disc_number > 0);
ALTER TABLE source_metadata ADD COLUMN track_number INTEGER CHECK(track_number > 0);
ALTER TABLE source_metadata ADD COLUMN inspection_version INTEGER NOT NULL DEFAULT 0;
CREATE INDEX source_metadata_release ON source_metadata(release_id, disc_number, track_number, source_id);
