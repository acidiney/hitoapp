use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["users"];

const REPLACED: &[(&str, &str)] = &[("table", "users")];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(86).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    let mut changed = Vec::new();
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0086 must not remove prior schema objects");
        if current != object {
            changed.push((object.0.as_str(), object.1.as_str()));
        }
    }
    let mut expected = REPLACED.to_vec();
    changed.sort_unstable();
    expected.sort_unstable();
    assert_eq!(changed, expected, "0086 must change only the users table");
    assert_eq!(
        before.len(),
        after.len(),
        "0086 must add no schema object and remove none"
    );
    connection
}
