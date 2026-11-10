mod catalog;
mod preservation;
mod semantics;
mod work;

use sqlx::{Connection, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::{borrow::Cow, path::Path};

async fn connect(path: &Path, version: i64) -> SqliteConnection {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    through(version).run(&mut connection).await.unwrap();
    connection
}

fn through(version: i64) -> Migrator {
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= version)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
}

async fn seed(connection: &mut SqliteConnection) {
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash) VALUES ('owner','owner','owned-unused');
        INSERT INTO libraries(id,name,owner_id) VALUES ('library','Owned library','owner');
        INSERT INTO library_members(library_id,user_id) VALUES ('library','owner');",
    )
    .execute(connection)
    .await
    .unwrap();
}

async fn grow(connection: &mut SqliteConnection, first: i64, last: i64) {
    sqlx::query("WITH RECURSIVE n(i) AS (SELECT ? UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO recordings(id,library_id,title,artist) SELECT 'recording-'||i,'library','Owned track','Owned recording artist' FROM n")
        .bind(first).bind(last).execute(&mut *connection).await.unwrap();
    sqlx::query("WITH RECURSIVE n(i) AS (SELECT ? UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO sources(id,recording_id,provider,provider_id) SELECT 'source-'||i,'recording-'||i,'local','owned-'||i FROM n")
        .bind(first).bind(last).execute(connection).await.unwrap();
}
