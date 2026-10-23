ALTER TABLE automatic_source_decisions ADD COLUMN musical_metadata TEXT
    CHECK(musical_metadata IS NULL OR (
        json_valid(musical_metadata)
        AND length(CAST(musical_metadata AS BLOB)) <= 32768
    ));
