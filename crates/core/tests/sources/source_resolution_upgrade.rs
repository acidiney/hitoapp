use sqlx::{
    Connection, Row, SqlitePool, TypeInfo, ValueRef,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteRow},
};
use std::{borrow::Cow, collections::BTreeMap};

#[path = "../fixtures/collection_artwork_schema.rs"]
mod collection_artwork_schema;
#[path = "../fixtures/release_credit_schema.rs"]
mod release_credit_schema;

#[derive(PartialEq, Eq)]
enum Cell {
    Null,
    Integer(i64),
    Real(u64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

type Rows = Vec<Vec<Cell>>;

#[tokio::test]
async fn automatic_source_upgrade_preserves_populated_binding_history_and_dependents() {
    let directory = tempfile::tempdir().unwrap();
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(directory.path().join("hito.db"))
            .create_if_missing(true)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 35)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../fixtures/source_resolution_upgrade.sql"))
        .execute(&pool)
        .await
        .unwrap();
    assert_integrity(&pool).await;
    let tables = snapshot_queries(&pool).await;
    let before = snapshots(&pool, &tables).await;
    let before_schema = schema(&pool).await;
    let before_indexes = indexes(&pool, "provider_items").await;
    let migrations = migration_history(&pool).await;
    let before_authorities = authorities(&pool).await;
    assert!(
        before_authorities.iter().all(|(_, rows)| rows.len() == 1),
        "each populated authority view must have a live fixture"
    );
    let prior_version: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(prior_version, 35);
    let prior_high_water: i64 =
        sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name='metadata_observations'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(prior_high_water, 900007);

    let mut baseline = pool.acquire().await.unwrap();
    let mut reference = release_credit_schema::reference(&mut baseline).await;
    let declared_release: BTreeMap<String, String> = sqlx::query_as(
        "SELECT name,sql FROM sqlite_schema WHERE type IN ('trigger','view') ORDER BY name",
    )
    .fetch_all(&mut reference)
    .await
    .unwrap()
    .into_iter()
    .collect();
    reference.close().await.unwrap();
    drop(baseline);
    let mut reference = collection_artwork_schema::reference().await;
    let declared_artwork = collection_artwork_schema::schema(&mut reference).await;
    reference.close().await.unwrap();

    all.run(&pool).await.unwrap();

    let after = snapshots(&pool, &tables).await;
    for (table, original) in &before {
        assert!(
            after.get(table) == Some(original),
            "migration changed typed rows or rowids in {table}"
        );
    }
    let after_schema = schema(&pool).await;
    for (name, (kind, original)) in before_schema {
        let definition = match name.as_str() {
            "current_download_authority" => declared_download_definition("VIEW", &name, ";"),
            "download_intent_identity" => declared_download_definition("TRIGGER", &name, "END;"),
            "release_artist_insert"
            | "release_artist_update"
            | "artist_releases"
            | "account_artist_releases" => declared_release.get(&name).unwrap().clone(),
            "artwork_fetch_identity"
            | "recording_artwork_owner"
            | "recording_artwork_update"
            | "current_artwork_fetch_authority" => declared_artwork
                .iter()
                .find(|object| object.1 == name)
                .unwrap()
                .3
                .clone()
                .unwrap(),
            _ => original,
        };
        let expected = (kind.clone(), definition);
        assert!(
            after_schema.get(&name) == Some(&expected),
            "migration changed retained {kind} {name} beyond its exact declared expectation"
        );
    }
    assert!(
        after_schema.get("provider_release_appearances")
            == Some(&(
                "view".to_owned(),
                declared_release
                    .get("provider_release_appearances")
                    .unwrap()
                    .clone(),
            )),
        "view provider_release_appearances must match its exact declaration"
    );
    assert!(
        indexes(&pool, "provider_items").await == before_indexes,
        "provider identity indexes must retain their definitions"
    );
    assert!(
        migration_history(&pool).await == migrations,
        "prior migration checksums and receipts must be retained"
    );
    assert!(
        authorities(&pool).await == before_authorities,
        "existing authority views must preserve their rows"
    );
    let version: i64 =
        sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(version, collection_artwork_schema::latest_version());
    collection_artwork_schema::assert_current(&mut pool.acquire().await.unwrap()).await;
    for table in [
        "recording_source_choices",
        "source_resolutions",
        "source_resolution_receipts",
        "automatic_source_decisions",
        "source_choice_receipts",
        "source_choice_reset_receipts",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {}", quoted(table)))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count, 0,
            "upgrade must not invent choices, resolutions or automatic decisions"
        );
    }
    let protected: i64 =
        sqlx::query_scalar("SELECT count(*) FROM provider_items WHERE evidence='user_confirmed'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        protected, 2,
        "active and retired human evidence must survive"
    );
    assert_integrity(&pool).await;
    assert_binding_guards(&pool).await;
    let next_sequence: i64 = sqlx::query_scalar("INSERT INTO metadata_observations(id,recording_id,provider,origin,title,artist,duration_ms) VALUES ('migration-after-observation','migration-recording','owned','provider_response','Owned after track','Owned artist',12000) RETURNING sequence")
        .fetch_one(&pool).await.unwrap();
    assert!(
        next_sequence > prior_high_water,
        "observation sequence must not reuse deleted history"
    );
    assert_integrity(&pool).await;
    pool.close().await;
}

async fn snapshot_queries(pool: &SqlitePool) -> Vec<(String, String)> {
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' AND name!='_sqlx_migrations' ORDER BY name")
        .fetch_all(pool).await.unwrap();
    let mut queries = Vec::new();
    for table in tables {
        let without_rowid: i64 =
            sqlx::query_scalar("SELECT wr FROM pragma_table_list WHERE schema='main' AND name=?")
                .bind(&table)
                .fetch_one(pool)
                .await
                .unwrap();
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_xinfo(?) WHERE hidden!=1 ORDER BY cid",
        )
        .bind(&table)
        .fetch_all(pool)
        .await
        .unwrap();
        let columns = columns
            .iter()
            .map(|column| quoted(column))
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
        let query = format!("SELECT {selected} FROM {} ORDER BY {order}", quoted(&table));
        queries.push((table, query));
    }
    queries
}

async fn snapshots(pool: &SqlitePool, tables: &[(String, String)]) -> BTreeMap<String, Rows> {
    let mut result = BTreeMap::new();
    for (table, query) in tables {
        let rows = sqlx::query(query)
            .fetch_all(pool)
            .await
            .unwrap()
            .iter()
            .map(cells)
            .collect();
        result.insert(table.clone(), rows);
    }
    result
}

fn cells(row: &SqliteRow) -> Vec<Cell> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(index, _)| {
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
}

async fn schema(pool: &SqlitePool) -> BTreeMap<String, (String, String)> {
    sqlx::query("SELECT name,type,sql FROM sqlite_schema WHERE type IN ('trigger','view') AND sql IS NOT NULL ORDER BY name")
        .fetch_all(pool).await.unwrap().into_iter().map(|row| (row.get("name"), (row.get("type"), row.get("sql")))).collect()
}

async fn indexes(pool: &SqlitePool, table: &str) -> BTreeMap<String, Rows> {
    let mut result = BTreeMap::new();
    let listed = sqlx::query(
        "SELECT name,\"unique\",origin,partial FROM pragma_index_list(?) ORDER BY name",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .unwrap();
    for row in listed {
        let name: String = row.get("name");
        let mut rows = vec![cells(&row)];
        rows.extend(
            sqlx::query("SELECT * FROM pragma_index_xinfo(?) ORDER BY seqno")
                .bind(&name)
                .fetch_all(pool)
                .await
                .unwrap()
                .iter()
                .map(cells),
        );
        result.insert(name, rows);
    }
    result
}

async fn migration_history(pool: &SqlitePool) -> Rows {
    sqlx::query("SELECT rowid,version,description,installed_on,success,checksum,execution_time FROM _sqlx_migrations WHERE version<=35 ORDER BY version")
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(cells)
        .collect()
}

async fn authorities(pool: &SqlitePool) -> BTreeMap<String, Rows> {
    let mut result = BTreeMap::new();
    for name in [
        "current_artwork_fetch_authority",
        "current_download_authority",
        "current_source_preparation_authority",
    ] {
        result.insert(
            name.to_owned(),
            sqlx::query(&format!("SELECT * FROM {} ORDER BY id", quoted(name)))
                .fetch_all(pool)
                .await
                .unwrap()
                .iter()
                .map(cells)
                .collect(),
        );
    }
    result
}

async fn assert_integrity(pool: &SqlitePool) {
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(pool)
            .await
            .unwrap()
            .is_empty(),
        "foreign key integrity must hold"
    );
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(integrity == "ok", "database integrity must hold");
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' AND sql LIKE '%USING fts5%' ORDER BY name").fetch_all(pool).await.unwrap();
    assert_eq!(tables.len(), 6, "exercise every current full-text index");
    for table in tables {
        sqlx::query(&format!(
            "INSERT INTO {}({},rank) VALUES ('integrity-check',1)",
            quoted(&table),
            quoted(&table)
        ))
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn assert_binding_guards(pool: &SqlitePool) {
    for statement in [
        "UPDATE provider_items SET evidence='provider_identity' WHERE id='migration-manual'",
        "UPDATE provider_items SET provider_recording_id='changed' WHERE id='migration-primary'",
        "DELETE FROM provider_items WHERE id='migration-retired'",
        "UPDATE recordings SET metadata_binding_id='migration-manual' WHERE id='migration-recording'",
        "UPDATE recordings SET library_id='migration-other-library' WHERE id='migration-recording'",
        "INSERT INTO provider_items(id,installation_id,provider_recording_id,recording_id,role,evidence) VALUES ('migration-wrong-owner','migration-installation','owned-wrong','migration-other-recording','source','automatic_policy')",
    ] {
        assert!(
            sqlx::query(statement).execute(pool).await.is_err(),
            "retained identity and ownership guards must reject invalid writes"
        );
    }
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,ready) VALUES ('migration-duplicate-source','migration-recording','plugin','owned-duplicate-source',0)").execute(&mut *transaction).await.unwrap();
    assert!(sqlx::query("INSERT INTO provider_sources(source_id,binding_id,installation_id,recording_id,provider_source_id,label,generation) VALUES ('migration-duplicate-source','migration-manual','migration-installation','migration-recording','owned-source-primary','Owned duplicate',4)").execute(&mut *transaction).await.is_err(), "source identity cannot move between active bindings");
    sqlx::query(
        "UPDATE provider_items SET retired=1,revision=revision+1 WHERE id='migration-manual'",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("INSERT INTO provider_items(id,installation_id,provider_recording_id,recording_id,role,evidence) VALUES ('migration-new-automatic','migration-installation','owned-new-automatic','migration-recording','source','automatic_policy')")
        .execute(&mut *transaction).await.unwrap();
    let evidence: String = sqlx::query_scalar(
        "SELECT evidence FROM provider_items WHERE id='migration-new-automatic'",
    )
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert!(
        evidence == "automatic_policy",
        "new automatic evidence must remain distinct from human evidence"
    );
    transaction.rollback().await.unwrap();
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn declared_download_definition(kind: &str, name: &str, terminator: &str) -> String {
    let migration = include_str!("../../migrations/0044_podcast_download_origins.sql");
    let declaration = format!("CREATE {kind} {name} ");
    let body = migration
        .split(&declaration)
        .nth(1)
        .expect("the download origin migration must declare its changed schema object")
        .split(terminator)
        .next()
        .unwrap();
    let mut definition = format!("{declaration}{body}{terminator}");
    definition.pop();
    definition.trim().to_owned()
}
