use sqlx::SqliteConnection;

pub const REPLACED: &[(&str, &str)] = &[("table", "source_resolutions")];

const COLUMN: &str = r#"failure TEXT CHECK(failure IS NULL OR CASE
    WHEN typeof(failure)='text' AND length(CAST(failure AS BLOB))<=128 AND json_valid(failure)
    THEN COALESCE(outcome='provider_unavailable'
        AND json_type(failure)='object'
        AND json_type(failure,'$.reason')='text'
        AND json_extract(failure,'$.reason') IN ('provider_unavailable','provider_refused','credentials_required','invalid_provider_response')
        AND json_type(failure,'$.retryable') IN ('true','false')
        AND json_remove(failure,'$.reason','$.retryable')='{}'
        AND json_extract(failure,'$.retryable')=(json_extract(failure,'$.reason')='provider_unavailable'),0)
    ELSE 0 END)"#;

pub fn with_failure(ddl: &str) -> String {
    let constraints = ddl
        .find(",\n    CHECK((lease_token IS NULL) = (lease_until IS NULL))")
        .unwrap();
    format!("{}, {}{}", &ddl[..constraints], COLUMN, &ddl[constraints..])
}

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    sqlx::raw_sql(&format!(
        "ALTER TABLE source_resolutions ADD COLUMN {COLUMN}"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    let after = super::schema(&mut connection).await;
    let expected = before
        .into_iter()
        .map(|mut object| {
            if object.0 == "table" && object.1 == "source_resolutions" {
                object.3 = object.3.map(|ddl| with_failure(&ddl));
            }
            object
        })
        .collect::<Vec<_>>();
    assert_eq!(
        after, expected,
        "0098 may only append the declared nullable failure column"
    );
    connection
}
