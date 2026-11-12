use sqlx::SqliteConnection;

pub const REPLACED: &[(&str, &str)] = &[
    ("trigger", "source_artist_insert"),
    ("trigger", "source_artist_update"),
];

pub fn definition(name: &str) -> String {
    let event = match name {
        "source_artist_insert" => "INSERT",
        "source_artist_update" => "UPDATE OF artist, source_id",
        _ => panic!("only the two source artist triggers change in 0093"),
    };
    format!(
        "CREATE TRIGGER {name} AFTER {event} ON source_metadata BEGIN\n    INSERT INTO artists SELECT lower(hex(randomblob(16))), r.library_id, new.artist\n    FROM sources s INDEXED BY sources_identity_recording\n    CROSS JOIN recordings r INDEXED BY sqlite_autoindex_recordings_1 ON r.id = s.recording_id\n    WHERE s.id = new.source_id ON CONFLICT(library_id, name) DO NOTHING;\nEND"
    )
}

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(93).run(&mut connection).await.unwrap();
    assert_replacement(&mut connection, before).await;
    connection
}

pub async fn assert_replacement(
    connection: &mut SqliteConnection,
    before: Vec<super::SchemaObject>,
) {
    let expected = before
        .into_iter()
        .map(|mut object| {
            if REPLACED
                .iter()
                .any(|(kind, name)| *kind == object.0 && *name == object.1)
            {
                object.3 = Some(definition(&object.1));
            }
            object
        })
        .collect::<Vec<_>>();
    let after = super::schema(connection).await;
    assert_eq!(
        after, expected,
        "0093 must replace exactly the two source artist trigger definitions and retain every other schema object"
    );
    for (_, name) in REPLACED {
        assert!(
            after
                .iter()
                .any(|object| object.0 == "trigger" && object.1 == *name)
        );
    }
}
