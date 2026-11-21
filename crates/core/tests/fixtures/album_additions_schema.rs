use sqlx::SqliteConnection;

const MIGRATION: &str = include_str!("../../migrations/0101_album_additions.sql");

pub const REPLACED: &[(&str, &str)] = &[
    ("view", "provider_release_appearances"),
    ("view", "artist_releases"),
    ("view", "account_artist_releases"),
];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "album_additions"),
    ("index", "sqlite_autoindex_album_additions_1"),
    ("index", "sqlite_autoindex_album_additions_2"),
    ("table", "catalog_appearances"),
    ("index", "sqlite_autoindex_catalog_appearances_1"),
    ("index", "sqlite_autoindex_catalog_appearances_2"),
    ("index", "catalog_appearance_release"),
    ("index", "catalog_appearance_addition"),
];

fn statement(kind: &str, name: &str) -> String {
    let prefix = format!("CREATE {} {name}", kind.to_uppercase());
    let start = MIGRATION
        .find(&prefix)
        .unwrap_or_else(|| panic!("0101 must declare {kind} {name}"));
    let end = MIGRATION[start..].find(';').unwrap() + start;
    MIGRATION[start..end].to_owned()
}

fn table_of(kind: &str, name: &str) -> String {
    match (kind, name) {
        ("index", "sqlite_autoindex_album_additions_1" | "sqlite_autoindex_album_additions_2") => {
            "album_additions".into()
        }
        ("index", "catalog_appearance_release" | "catalog_appearance_addition")
        | (
            "index",
            "sqlite_autoindex_catalog_appearances_1" | "sqlite_autoindex_catalog_appearances_2",
        ) => "catalog_appearances".into(),
        _ => name.into(),
    }
}

pub fn declarations() -> Vec<super::SchemaObject> {
    REPLACED
        .iter()
        .chain(ADDED)
        .map(|(kind, name)| {
            let sql = (!name.starts_with("sqlite_autoindex_")).then(|| statement(kind, name));
            ((*kind).into(), (*name).into(), table_of(kind, name), sql)
        })
        .collect()
}

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    assert!(REPLACED.iter().all(|(kind, name)| {
        before
            .iter()
            .any(|object| object.0 == *kind && object.1 == *name)
    }));
    assert!(ADDED.iter().all(|(kind, name)| {
        before
            .iter()
            .all(|object| object.0 != *kind || object.1 != *name)
    }));
    sqlx::raw_sql(MIGRATION)
        .execute(&mut connection)
        .await
        .unwrap();
    let declarations = declarations();
    let mut expected: Vec<_> = before
        .into_iter()
        .map(|object| {
            declarations
                .iter()
                .find(|current| {
                    current.0 == object.0
                        && current.1 == object.1
                        && REPLACED
                            .iter()
                            .any(|(kind, name)| object.0 == *kind && object.1 == *name)
                })
                .cloned()
                .unwrap_or(object)
        })
        .collect();
    expected.extend(declarations.into_iter().filter(|object| {
        ADDED
            .iter()
            .any(|(kind, name)| object.0 == *kind && object.1 == *name)
    }));
    expected.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
    assert_eq!(
        super::schema(&mut connection).await,
        expected,
        "0101 must add only the album addition tables and replace only the three release appearance views"
    );
    connection
}
