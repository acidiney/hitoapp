use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["artist_name_checks", "artist_matching_jobs"];

pub const ADDED: &[(&str, &str)] = &[];

/// The columns 0072 appends, by table.
const COLUMNS: &[(&str, &str)] = &[
    ("artist_name_checks", "rule INTEGER NOT NULL DEFAULT 1"),
    (
        "artist_matching_jobs",
        "unavailable INTEGER NOT NULL DEFAULT 0 CHECK(unavailable IN (0,1))",
    ),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(72).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    assert_eq!(
        before.len(),
        after.len(),
        "0072 must add no schema object and remove none"
    );
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0072 must not remove prior schema objects");
        match COLUMNS.iter().find(|(table, _)| *table == object.1) {
            Some((_, column)) if object.0 == "table" => {
                let ddl = object.3.as_deref().unwrap().trim_end();
                let declared = format!("{}, {column})", ddl.strip_suffix(')').unwrap());
                assert_eq!(
                    current.3.as_deref(),
                    Some(declared.as_str()),
                    "0072 must only append its column to {}",
                    object.1
                );
            }
            _ => assert_eq!(current, object, "0072 must change only its two tables"),
        }
    }
    connection
}
