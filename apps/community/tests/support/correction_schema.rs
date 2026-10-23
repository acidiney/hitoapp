use sqlx::{Row, SqliteConnection};

type SchemaObject = (String, String, String, Option<String>);
type ForeignKeys = Vec<(i64, i64, String, String, String, String, String, String)>;
type Columns = Vec<(i64, String, String, i64, Option<String>, i64)>;

pub fn objects() -> Vec<SchemaObject> {
    let mut objects = vec![
        (
            "table".into(),
            "correction_suggestions".into(),
            "correction_suggestions".into(),
            Some(SUGGESTIONS.into()),
        ),
        (
            "table".into(),
            "correction_review_receipts".into(),
            "correction_review_receipts".into(),
            Some(REVIEWS.into()),
        ),
        (
            "table".into(),
            "sqlite_sequence".into(),
            "sqlite_sequence".into(),
            Some("CREATE TABLE sqlite_sequence(name,seq)".into()),
        ),
    ];
    for (name, columns) in [
        ("corrections_reporter_created", "reporter_id, created_at"),
        ("corrections_status_sequence", "status, sequence"),
        (
            "corrections_reporter_status_sequence",
            "reporter_id, status, sequence",
        ),
    ] {
        objects.push((
            "index".into(),
            name.into(),
            "correction_suggestions".into(),
            Some(format!(
                "CREATE INDEX {name} ON correction_suggestions ({columns})"
            )),
        ));
    }
    for table in ["correction_suggestions", "correction_review_receipts"] {
        for index in 1..=2 {
            objects.push((
                "index".into(),
                format!("sqlite_autoindex_{table}_{index}"),
                table.into(),
                None,
            ));
        }
    }
    objects
}

pub async fn assert_empty_structure(connection: &mut SqliteConnection) {
    for table in [
        "correction_suggestions",
        "correction_review_receipts",
        "sqlite_sequence",
    ] {
        assert!(
            sqlx::query(&format!("SELECT * FROM {table}"))
                .fetch_all(&mut *connection)
                .await
                .unwrap()
                .is_empty()
        );
    }
    let keys: ForeignKeys = sqlx::query_as("PRAGMA foreign_key_list(correction_suggestions)")
        .fetch_all(&mut *connection)
        .await
        .unwrap();
    assert_eq!(
        keys,
        [(
            0,
            0,
            "reporters".into(),
            "reporter_id".into(),
            "id".into(),
            "NO ACTION".into(),
            "CASCADE".into(),
            "NONE".into()
        )]
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_list(correction_review_receipts)")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty()
    );
    for (table, expected) in [
        (
            "correction_suggestions",
            &[
                ("sequence", "INTEGER", 0, None, 1),
                ("correction_id", "TEXT", 1, None, 0),
                ("reporter_id", "TEXT", 1, None, 0),
                ("submission_id", "TEXT", 1, None, 0),
                ("left_provider", "TEXT", 1, None, 0),
                ("left_recording_id", "TEXT", 1, None, 0),
                ("right_provider", "TEXT", 1, None, 0),
                ("right_recording_id", "TEXT", 1, None, 0),
                ("relation", "TEXT", 1, None, 0),
                ("status", "TEXT", 1, Some("'pending'"), 0),
                ("revision", "INTEGER", 1, Some("1"), 0),
                ("created_at", "INTEGER", 1, Some("unixepoch()"), 0),
            ][..],
        ),
        (
            "correction_review_receipts",
            &[
                ("request_id", "TEXT", 1, None, 1),
                ("correction_id", "TEXT", 1, None, 0),
                ("expected_revision", "INTEGER", 1, None, 0),
                ("result_revision", "INTEGER", 1, None, 0),
                ("decision", "TEXT", 1, None, 0),
                ("created_at", "INTEGER", 1, Some("unixepoch()"), 0),
            ][..],
        ),
    ] {
        let columns: Columns = sqlx::query_as(&format!("PRAGMA table_info({table})"))
            .fetch_all(&mut *connection)
            .await
            .unwrap();
        let expected: Columns = expected
            .iter()
            .enumerate()
            .map(|(index, (name, kind, required, default, primary))| {
                (
                    index as i64,
                    (*name).into(),
                    (*kind).into(),
                    *required,
                    default.map(str::to_owned),
                    *primary,
                )
            })
            .collect();
        assert_eq!(columns, expected);
    }
    for (index, expected) in [
        (
            "corrections_reporter_created",
            vec!["reporter_id", "created_at"],
        ),
        ("corrections_status_sequence", vec!["status", "sequence"]),
        (
            "corrections_reporter_status_sequence",
            vec!["reporter_id", "status", "sequence"],
        ),
    ] {
        let rows = sqlx::query(&format!("PRAGMA index_info({index})"))
            .fetch_all(&mut *connection)
            .await
            .unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| row.get::<String, _>("name"))
                .collect::<Vec<_>>(),
            expected
        );
    }
}

const SUGGESTIONS: &str = r#"CREATE TABLE correction_suggestions (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    correction_id TEXT UNIQUE NOT NULL,
    reporter_id TEXT NOT NULL REFERENCES reporters(id) ON DELETE CASCADE,
    submission_id TEXT NOT NULL,
    left_provider TEXT NOT NULL CHECK (left_provider IN ('youtube', 'jamendo')),
    left_recording_id TEXT NOT NULL,
    right_provider TEXT NOT NULL CHECK (right_provider IN ('youtube', 'jamendo')),
    right_recording_id TEXT NOT NULL,
    relation TEXT NOT NULL CHECK (relation = 'same_recording'),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision BETWEEN 1 AND 9007199254740991),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (reporter_id, submission_id),
    CHECK (left_provider < right_provider OR (left_provider = right_provider AND left_recording_id < right_recording_id))
)"#;

const REVIEWS: &str = r#"CREATE TABLE correction_review_receipts (
    request_id TEXT PRIMARY KEY NOT NULL,
    correction_id TEXT NOT NULL,
    expected_revision INTEGER NOT NULL CHECK (expected_revision BETWEEN 1 AND 9007199254740990),
    result_revision INTEGER NOT NULL CHECK (result_revision = expected_revision + 1),
    decision TEXT NOT NULL CHECK (decision IN ('approve', 'reject')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (correction_id, result_revision)
)"#;
