use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["artist_association_name_matches"];

pub const ADDED: &[(&str, &str)] = &[];

type Column = (i64, String, String, i64, Option<String>, i64, i64);

/// The column 0074 appends: name, declared type, not-null flag, default and its exact
/// declaration text.
const COLUMNS: &[(&str, &str, &str, i64, &str, &str)] = &[(
    "artist_association_name_matches",
    "rule",
    "INTEGER",
    1,
    "2",
    "rule INTEGER NOT NULL DEFAULT 2",
)];

async fn columns(connection: &mut SqliteConnection, table: &str) -> Vec<Column> {
    sqlx::query_as("SELECT cid,name,type,\"notnull\",dflt_value,pk,hidden FROM pragma_table_xinfo(?) ORDER BY cid")
        .bind(table)
        .fetch_all(connection)
        .await
        .unwrap()
}

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    let mut before_columns = Vec::new();
    for (table, ..) in COLUMNS {
        before_columns.push(columns(&mut connection, table).await);
    }
    super::through(74).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    assert_eq!(
        before.len(),
        after.len(),
        "0074 must add no schema object and remove none"
    );
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0074 must not remove prior schema objects");
        if object.0 == "table" && COLUMNS.iter().any(|column| column.0 == object.1) {
            continue;
        }
        assert_eq!(current, object, "0074 must change only its one table");
    }
    for ((table, name, kind, not_null, default, declaration), old) in
        COLUMNS.iter().zip(before_columns)
    {
        let now = columns(&mut connection, table).await;
        assert_eq!(
            &now[..old.len()],
            old.as_slice(),
            "{table} keeps its columns"
        );
        assert_eq!(now.len(), old.len() + 1, "0074 adds one column to {table}");
        let added = now.last().unwrap();
        assert_eq!(
            (
                added.1.as_str(),
                added.2.as_str(),
                added.3,
                added.4.as_deref(),
                added.5
            ),
            (*name, *kind, *not_null, Some(*default), 0),
            "{table} gains exactly the declared column"
        );
        let ddl = after
            .iter()
            .find(|object| object.0 == "table" && object.1 == *table)
            .and_then(|object| object.3.as_deref())
            .unwrap();
        assert!(ddl.contains(declaration), "{table} declares {declaration}");
    }
    connection
}
