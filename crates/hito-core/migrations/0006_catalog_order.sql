CREATE INDEX recordings_catalog_order ON recordings(title COLLATE NOCASE,id,library_id);
