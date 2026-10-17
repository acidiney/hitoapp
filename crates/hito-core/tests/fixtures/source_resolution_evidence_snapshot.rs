use sqlx::{Row, SqliteConnection, TypeInfo, ValueRef};

#[derive(PartialEq, Eq)]
enum Cell {
    Null,
    Integer(i64),
    Real(u64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

pub(super) struct Projection {
    statement: String,
    rows: Vec<Vec<Cell>>,
}

pub(super) async fn capture(
    connection: &mut SqliteConnection,
    migration_version: i64,
) -> Vec<Projection> {
    let tables: Vec<(String, i64)> = sqlx::query_as(
        "SELECT schema.name,metadata.wr FROM sqlite_schema schema
         JOIN pragma_table_list metadata ON metadata.schema='main' AND metadata.name=schema.name
         WHERE schema.type='table' ORDER BY schema.name",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    let mut projections = Vec::new();
    for (table, without_rowid) in tables {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(&table)
                .fetch_all(&mut *connection)
                .await
                .unwrap();
        let columns = columns
            .iter()
            .map(|name| quoted(name))
            .collect::<Vec<_>>()
            .join(",");
        let selected = if without_rowid == 0 {
            format!("rowid,{columns}")
        } else {
            columns.clone()
        };
        let order = if without_rowid == 0 {
            "rowid"
        } else {
            &columns
        };
        let filter = if table == "_sqlx_migrations" {
            format!(" WHERE version<={migration_version}")
        } else {
            String::new()
        };
        let statement = format!(
            "SELECT {selected} FROM {}{filter} ORDER BY {order}",
            quoted(&table)
        );
        let rows = typed_rows(connection, &statement).await;
        projections.push(Projection { statement, rows });
    }
    projections
}

async fn typed_rows(connection: &mut SqliteConnection, statement: &str) -> Vec<Vec<Cell>> {
    sqlx::query(statement)
        .fetch_all(connection)
        .await
        .unwrap()
        .iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|index| {
                    let value = row.try_get_raw(index).unwrap();
                    if value.is_null() {
                        return Cell::Null;
                    }
                    match value.type_info().name() {
                        "INTEGER" | "BOOLEAN" => Cell::Integer(row.try_get(index).unwrap()),
                        "REAL" => Cell::Real(row.try_get::<f64, _>(index).unwrap().to_bits()),
                        "TEXT" => Cell::Text(row.try_get::<String, _>(index).unwrap().into_bytes()),
                        "BLOB" => Cell::Blob(row.try_get(index).unwrap()),
                        _ => panic!("owned migration fixture has an unsupported SQLite type"),
                    }
                })
                .collect()
        })
        .collect()
}

pub(super) async fn assert_preserved(connection: &mut SqliteConnection, before: &[Projection]) {
    for projection in before {
        assert!(
            typed_rows(connection, &projection.statement).await == projection.rows,
            "migration must retain every prior typed column, row and rowid, including migration receipts"
        );
    }
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
