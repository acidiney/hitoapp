use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["playlist_follows"];

type Column = (i64, String, String, i64, Option<String>, i64, i64);

const ADDED: &[(&str, &str)] = &[
    ("approved_capabilities", "approved_capabilities TEXT"),
    ("approved_credential", "approved_credential TEXT"),
];

async fn columns(connection: &mut SqliteConnection) -> Vec<Column> {
    sqlx::query_as("SELECT cid,name,type,\"notnull\",dflt_value,pk,hidden FROM pragma_table_xinfo('playlist_follows') ORDER BY cid")
        .fetch_all(connection)
        .await
        .unwrap()
}

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    let before_columns = columns(&mut connection).await;
    super::through(79).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    assert_eq!(
        before.len(),
        after.len(),
        "0079 must add no schema object and remove none"
    );
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0079 must not remove prior schema objects");
        if object.0 == "table" && object.1 == "playlist_follows" {
            continue;
        }
        assert_eq!(current, object, "0079 must change only playlist_follows");
    }
    let now = columns(&mut connection).await;
    assert_eq!(
        &now[..before_columns.len()],
        before_columns.as_slice(),
        "playlist_follows keeps its columns"
    );
    assert_eq!(now.len(), before_columns.len() + ADDED.len());
    for ((name, declaration), column) in ADDED.iter().zip(&now[before_columns.len()..]) {
        assert_eq!(
            (
                column.1.as_str(),
                column.2.as_str(),
                column.3,
                column.4.as_deref(),
                column.5
            ),
            (*name, "TEXT", 0, None, 0)
        );
        let ddl = after
            .iter()
            .find(|object| object.0 == "table" && object.1 == "playlist_follows")
            .and_then(|object| object.3.as_deref())
            .unwrap();
        assert!(
            ddl.contains(declaration),
            "playlist_follows declares {declaration}"
        );
    }
    connection
}
