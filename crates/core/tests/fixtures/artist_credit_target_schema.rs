use sqlx::{Connection, SqliteConnection};

pub const REPLACED: &[(&str, &str)] = &[
    ("table", "recording_artist_associations"),
    ("trigger", "recording_artist_association_scope"),
];

pub const ADDED: &[(&str, &str)] = &[
    ("trigger", "recording_artist_identity_choice_target_insert"),
    ("trigger", "recording_artist_identity_choice_target_update"),
];

const DECLARATIONS: &[(&str, &str, &str, &str)] = &[
    (
        "table",
        "recording_artist_associations",
        "recording_artist_associations",
        r#"CREATE TABLE "recording_artist_associations" (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    artist_id TEXT NOT NULL,
    source_id TEXT REFERENCES sources(id),
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id),
    role TEXT NOT NULL CHECK(role = 'recording_artist'),
    provider_kind TEXT NOT NULL CHECK(provider_kind IN ('builtin_musicbrainz','http_plugin')),
    plugin_installation_id TEXT REFERENCES plugin_installations(id),
    provider_key TEXT GENERATED ALWAYS AS (
        CASE WHEN provider_kind = 'builtin_musicbrainz' THEN 'builtin:musicbrainz'
             ELSE 'http:' || plugin_installation_id END
    ) STORED,
    provider_artist_id TEXT NOT NULL CHECK(length(CAST(provider_artist_id AS BLOB)) BETWEEN 1 AND 256),
    evidence TEXT NOT NULL CHECK(evidence = 'user_confirmed'),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision = 1),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    credit_target TEXT NOT NULL DEFAULT 'whole_credit'
        CHECK(credit_target IN ('whole_credit','main_member','featured_member')),
    FOREIGN KEY(user_id,artist_id) REFERENCES account_artists(user_id,id),
    CHECK((provider_kind = 'builtin_musicbrainz' AND plugin_installation_id IS NULL)
        OR (provider_kind = 'http_plugin' AND plugin_installation_id IS NOT NULL)),
    UNIQUE(user_id,recording_id,artist_id,observation_id,role,provider_key,provider_artist_id,credit_target)
)"#,
    ),
    (
        "trigger",
        "recording_artist_association_scope",
        "recording_artist_associations",
        r#"CREATE TRIGGER recording_artist_association_scope BEFORE INSERT ON recording_artist_associations
WHEN NOT EXISTS (
    SELECT 1 FROM recordings r
    JOIN libraries library ON library.id = r.library_id
    JOIN library_members access ON access.library_id = r.library_id AND access.user_id = new.user_id
    JOIN account_artists artist ON artist.user_id = access.user_id AND artist.id = new.artist_id AND artist.library_id = r.library_id
    LEFT JOIN account_recording_credits credit ON credit.user_id = access.user_id AND credit.recording_id = r.id AND credit.artist_id = artist.id
    LEFT JOIN account_recording_credit_members member ON member.user_id = access.user_id
        AND member.recording_id = r.id AND member.artist_id = artist.id
    JOIN metadata_observations observation ON observation.id = new.observation_id
        AND observation.id = r.metadata_observation_id AND observation.recording_id = r.id
        AND observation.source_id IS new.source_id
    LEFT JOIN sources source ON source.id = observation.source_id AND source.recording_id = r.id
    LEFT JOIN source_metadata source_state ON source_state.source_id = source.id AND source_state.observation_id = observation.id
    LEFT JOIN provider_items primary_binding ON primary_binding.id = r.metadata_binding_id
        AND primary_binding.id = observation.provider_binding_id AND primary_binding.recording_id = r.id
        AND primary_binding.installation_id = observation.plugin_installation_id
        AND primary_binding.provider_recording_id = observation.provider_recording_id
        AND primary_binding.role = 'primary' AND primary_binding.retired = 0
    LEFT JOIN plugin_installations origin ON origin.id = primary_binding.installation_id AND origin.user_id = access.user_id
    WHERE r.id = new.recording_id AND r.library_id = new.library_id
        AND ((new.credit_target = 'whole_credit' AND credit.artist_id IS NOT NULL)
            OR (new.credit_target = 'main_member' AND member.role = 'main')
            OR (new.credit_target = 'featured_member' AND member.role = 'featured'))
        AND ((observation.source_id IS NOT NULL AND source.id IS NOT NULL AND source_state.observation_id = observation.id)
            OR (observation.source_id IS NULL AND observation.origin = 'provider_response'
                AND primary_binding.id IS NOT NULL AND origin.id IS NOT NULL AND library.owner_id = access.user_id))
) OR (new.plugin_installation_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM plugin_installations installation
    WHERE installation.id = new.plugin_installation_id AND installation.user_id = new.user_id
)) BEGIN
    SELECT RAISE(ABORT, 'Artist associations require their account credit and current observation');
END"#,
    ),
    (
        "trigger",
        "recording_artist_identity_choice_target_insert",
        "recording_artist_identity_choices",
        r#"CREATE TRIGGER recording_artist_identity_choice_target_insert BEFORE INSERT ON recording_artist_identity_choices
WHEN new.association_id IS NOT NULL AND EXISTS (
    SELECT 1 FROM recording_artist_associations association
    WHERE association.id=new.association_id AND association.credit_target<>'whole_credit'
) BEGIN
    SELECT RAISE(ABORT,'Recording identity choices require a whole-credit association');
END"#,
    ),
    (
        "trigger",
        "recording_artist_identity_choice_target_update",
        "recording_artist_identity_choices",
        r#"CREATE TRIGGER recording_artist_identity_choice_target_update BEFORE UPDATE ON recording_artist_identity_choices
WHEN new.association_id IS NOT NULL AND EXISTS (
    SELECT 1 FROM recording_artist_associations association
    WHERE association.id=new.association_id AND association.credit_target<>'whole_credit'
) BEGIN
    SELECT RAISE(ABORT,'Recording identity choices require a whole-credit association');
END"#,
    ),
];

pub fn declarations() -> Vec<super::SchemaObject> {
    DECLARATIONS
        .iter()
        .map(|(kind, name, table, sql)| {
            (
                (*kind).into(),
                (*name).into(),
                (*table).into(),
                Some((*sql).into()),
            )
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
    let mut transaction = connection.begin().await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/0096_artist_credit_targets.sql"
    ))
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let declarations = declarations();
    let mut expected: Vec<_> = before
        .into_iter()
        .map(|object| {
            if REPLACED
                .iter()
                .any(|(kind, name)| object.0 == *kind && object.1 == *name)
            {
                declarations
                    .iter()
                    .find(|current| current.0 == object.0 && current.1 == object.1)
                    .unwrap()
                    .clone()
            } else {
                object
            }
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
        "0096 must replace only the association table and scope, add only the two whole-credit choice guards, and retain every other exact schema object"
    );
    connection
}
