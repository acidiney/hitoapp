use sqlx::SqliteConnection;

const MIGRATION: &str = include_str!("../../migrations/0104_plugin_authorization_native.sql");

pub const REPLACED: &[(&str, &str)] = &[("table", "plugin_authorization_attempts")];

pub fn table() -> String {
    let start = MIGRATION
        .find("CREATE TABLE plugin_authorization_attempts (")
        .expect("0104 must declare the attempts table");
    let end = MIGRATION[start..].find(";\n").unwrap() + start;
    MIGRATION[start..end].to_owned()
}

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    sqlx::raw_sql(MIGRATION)
        .execute(&mut connection)
        .await
        .unwrap();
    let after = super::schema(&mut connection).await;
    assert!(after.iter().any(|object| object.0 == "table"
        && object.1 == "plugin_authorization_attempts"
        && object.3.as_deref() == Some(table().as_str())));
    let mut changed = Vec::new();
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0104 must not remove prior schema objects");
        if current != object {
            changed.push((object.0.as_str(), object.1.as_str()));
        }
    }
    assert_eq!(
        changed,
        REPLACED.to_vec(),
        "0104 must change only the plugin authorization attempts table"
    );
    assert_eq!(
        before.len(),
        after.len(),
        "0104 must add no schema object and remove none"
    );
    connection
}
