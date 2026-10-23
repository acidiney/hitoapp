CREATE INDEX recording_metadata_source_lookup ON recordings(metadata_source_id) WHERE metadata_source_id IS NOT NULL;
