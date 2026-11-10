use super::fixture::{RemoteServer, local};
use hito_core::{Browse, Credentials, DeleteAccount};
use sqlx::{Connection, SqliteConnection};

#[tokio::test]
async fn account_deletion_retires_only_its_owned_replica_and_private_state() {
    let (directory, services, admin) = local().await;
    let credentials = || Credentials {
        username: "erasable-listener".into(),
        password: "owned account deletion password".into(),
    };
    let user = services
        .identity
        .create_user(&admin, credentials())
        .await
        .unwrap();
    let authenticated = services.identity.login(credentials()).await.unwrap();
    let member = services
        .identity
        .authenticate(&authenticated.token)
        .await
        .unwrap();
    let remote = RemoteServer::start().await;
    let kept = services
        .replicas
        .connect(&admin, remote.request())
        .await
        .unwrap();
    let erased = services
        .replicas
        .connect(&member, remote.request())
        .await
        .unwrap();
    let lease = services
        .replicas
        .activate(&member, erased.id)
        .await
        .unwrap();
    services
        .identity
        .delete_account(
            &admin,
            &user.id,
            DeleteAccount {
                username: user.username,
            },
        )
        .await
        .unwrap();
    assert!(
        services
            .replicas
            .playlists(&member, &lease, Browse::default())
            .await
            .is_err()
    );
    assert!(
        !directory
            .path()
            .join("hito.replicas")
            .join(erased.id.to_string())
            .exists()
    );
    assert!(
        directory
            .path()
            .join("hito.replicas")
            .join(kept.id.to_string())
            .exists()
    );
    assert_eq!(
        services.replicas.connections(&admin).await.unwrap().len(),
        1
    );
    let mut database = SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table'")
            .fetch_all(&mut database)
            .await
            .unwrap();
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
            sqlx::query_as(&format!("PRAGMA table_info(\"{quoted}\")"))
                .fetch_all(&mut database)
                .await
                .unwrap();
        for (_, column, _, _, _, _) in columns {
            let column = column.replace('"', "\"\"");
            let mentions: i64 = sqlx::query_scalar(&format!(
                "SELECT count(*) FROM \"{quoted}\" WHERE instr(CAST(\"{column}\" AS TEXT),?)>0"
            ))
            .bind(&user.id)
            .fetch_one(&mut database)
            .await
            .unwrap();
            assert_eq!(mentions, 0, "erased account remains in {table}.{column}");
        }
    }
    services.replicas.close().await.unwrap();
}
