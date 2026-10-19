use super::fixture;
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) const TITLE: &str = "PRIVATE_MAINTENANCE_TITLE_CANARY";
pub(super) const TOKEN: &str = "PRIVATE_MAINTENANCE_TOKEN_CANARY";

pub(super) async fn library() -> fixture::Library {
    let library = fixture::Library::version_29("http://127.0.0.1:9/").await;
    let mut connection = fixture::connect(&library.database).await;
    sqlx::query("UPDATE recordings SET title=?")
        .bind(TITLE)
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    std::fs::write(library.directory.path().join("private-token-canary"), TOKEN).unwrap();
    library
}

#[derive(PartialEq, Eq)]
enum Cell {
    Null,
    Integer(i64),
    Real(u64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(PartialEq, Eq)]
struct Table {
    name: String,
    has_rowid: bool,
    columns: Vec<String>,
    rows: Vec<Vec<Cell>>,
}

type SchemaObject = (String, String, String, Option<String>);

#[derive(PartialEq, Eq)]
pub(super) struct State {
    schema: Vec<SchemaObject>,
    tables: Vec<Table>,
}

pub(super) async fn state(database: &Path) -> State {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(false)
            .read_only(true),
    )
    .await
    .unwrap();
    let schema: Vec<SchemaObject> =
        sqlx::query_as("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert!(
        schema.len() <= 524,
        "owned schema snapshot bound exceeded: objects={} tables={} indexes={} triggers={} views={}",
        schema.len(),
        schema.iter().filter(|object| object.0 == "table").count(),
        schema.iter().filter(|object| object.0 == "index").count(),
        schema.iter().filter(|object| object.0 == "trigger").count(),
        schema.iter().filter(|object| object.0 == "view").count(),
    );
    let tables: Vec<(String, bool)> = sqlx::query_as("SELECT name,wr=0 FROM pragma_table_list WHERE schema='main' AND type IN ('table','shadow','virtual') AND (name NOT LIKE 'sqlite_%' OR name='sqlite_sequence') ORDER BY name")
        .fetch_all(&mut connection).await.unwrap();
    assert!(tables.len() <= 128, "owned snapshot table bound exceeded");
    let mut state = Vec::new();
    for (name, has_rowid) in tables {
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_xinfo(?) WHERE hidden=0 ORDER BY cid",
        )
        .bind(&name)
        .fetch_all(&mut connection)
        .await
        .unwrap();
        assert!(
            !columns.is_empty() && columns.len() <= 256,
            "owned snapshot column bound exceeded"
        );
        let expressions = columns
            .iter()
            .map(|column| {
                let column = quoted(column);
                format!("typeof({column}),{column}")
            })
            .collect::<Vec<_>>()
            .join(",");
        let prefix = if has_rowid { "rowid," } else { "" };
        let order = if has_rowid {
            "rowid".to_owned()
        } else {
            columns
                .iter()
                .map(|column| {
                    let column = quoted(column);
                    format!("typeof({column}),{column} COLLATE BINARY")
                })
                .collect::<Vec<_>>()
                .join(",")
        };
        let result = sqlx::query(&format!(
            "SELECT {prefix}{expressions} FROM {} ORDER BY {order} LIMIT 1025",
            quoted(&name)
        ))
        .fetch_all(&mut connection)
        .await
        .unwrap();
        assert!(result.len() <= 1024, "owned snapshot row bound exceeded");
        let mut rows = Vec::new();
        for row in result {
            let offset = usize::from(has_rowid);
            let mut cells = if has_rowid {
                vec![Cell::Integer(row.get(0))]
            } else {
                Vec::new()
            };
            for index in 0..columns.len() {
                let storage = offset + index * 2;
                let value = storage + 1;
                cells.push(match row.get::<String, _>(storage).as_str() {
                    "null" => Cell::Null,
                    "integer" => Cell::Integer(row.get(value)),
                    "real" => Cell::Real(row.get::<f64, _>(value).to_bits()),
                    "text" => {
                        let value: String = row.get(value);
                        assert!(value.len() <= 1048576);
                        Cell::Text(value)
                    }
                    "blob" => {
                        let value: Vec<u8> = row.get(value);
                        assert!(value.len() <= 1048576);
                        Cell::Blob(value)
                    }
                    _ => panic!("unexpected SQLite storage class"),
                });
            }
            rows.push(cells);
        }
        state.push(Table {
            name,
            has_rowid,
            columns,
            rows,
        });
    }
    connection.close().await.unwrap();
    State {
        schema,
        tables: state,
    }
}

fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

pub(super) fn files(root: &Path, exclude_database: bool) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(
        root: &Path,
        directory: &Path,
        exclude_database: bool,
        entries: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
    ) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if exclude_database
                && directory == root
                && ["hito.db", "hito.db-wal", "hito.db-shm"]
                    .contains(&entry.file_name().to_str().unwrap())
            {
                continue;
            }
            assert!(
                entries.len() < 128 && path.strip_prefix(root).unwrap().components().count() <= 8,
                "owned file snapshot bound exceeded"
            );
            let kind = entry.file_type().unwrap();
            let value = if kind.is_dir() {
                None
            } else {
                assert!(
                    kind.is_file() && entry.metadata().unwrap().len() <= 4194304,
                    "owned file snapshot requires bounded regular files"
                );
                Some(std::fs::read(&path).unwrap())
            };
            entries.insert(path.strip_prefix(root).unwrap().to_owned(), value);
            if kind.is_dir() {
                visit(root, &path, exclude_database, entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, exclude_database, &mut entries);
    entries
}
