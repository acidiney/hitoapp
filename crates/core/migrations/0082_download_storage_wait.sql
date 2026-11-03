ALTER TABLE download_intents ADD COLUMN storage_wait INTEGER NOT NULL DEFAULT 0 CHECK(storage_wait IN (0,1));
