ALTER TABLE source_resolutions ADD COLUMN failure TEXT CHECK(failure IS NULL OR CASE
    WHEN typeof(failure)='text' AND length(CAST(failure AS BLOB))<=128 AND json_valid(failure)
    THEN COALESCE(outcome='provider_unavailable'
        AND json_type(failure)='object'
        AND json_type(failure,'$.reason')='text'
        AND json_extract(failure,'$.reason') IN ('provider_unavailable','provider_refused','credentials_required','invalid_provider_response')
        AND json_type(failure,'$.retryable') IN ('true','false')
        AND json_remove(failure,'$.reason','$.retryable')='{}'
        AND json_extract(failure,'$.retryable')=(json_extract(failure,'$.reason')='provider_unavailable'),0)
    ELSE 0 END);
