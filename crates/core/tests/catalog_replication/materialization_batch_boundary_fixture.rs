use crate::{
    CatalogSnapshotMode, Credentials, Error, ServerRelayObservation, test_support::support,
};

pub(crate) struct SourceExpectation {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) duration_ms: i64,
    pub(crate) relay: ServerRelayObservation,
    pub(crate) release: ReleaseExpectation,
}

pub(crate) struct ReleaseExpectation {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) disc_number: Option<i64>,
    pub(crate) track_number: Option<i64>,
}

pub(crate) struct RecordingExpectation {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) duration_ms: i64,
    pub(crate) sources: Vec<SourceExpectation>,
}

pub(crate) fn release(number: usize) -> ReleaseExpectation {
    ReleaseExpectation {
        id: format!("boundary-release-{number}"),
        title: format!("Release {number}"),
        artist: (number != 1).then(|| format!("Release Artist {number}")),
        disc_number: Some(number as i64 + 1),
        track_number: Some(number as i64 + 7),
    }
}

pub(crate) struct SourceSeed {
    pub(crate) recording_id: String,
    pub(crate) source_id: String,
    pub(crate) device_id: Option<String>,
    pub(crate) ready: bool,
    pub(crate) release_id: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) duration_ms: i64,
    pub(crate) disc_number: Option<i64>,
    pub(crate) track_number: Option<i64>,
}

pub(crate) async fn insert_source(services: &crate::Services, seed: SourceSeed) {
    sqlx::query(
        "INSERT INTO sources(id,recording_id,provider,provider_id,device_id,ready)
         VALUES (?,?,'local',?,?,?)",
    )
    .bind(&seed.source_id)
    .bind(&seed.recording_id)
    .bind(format!("provider-{}", seed.source_id))
    .bind(seed.device_id)
    .bind(seed.ready)
    .execute(&services.catalog_replication.store.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,disc_number,track_number)
         VALUES (?,?,?,?,?,?,?)",
    )
    .bind(seed.source_id)
    .bind(seed.release_id)
    .bind(seed.title)
    .bind(seed.artist)
    .bind(seed.duration_ms)
    .bind(seed.disc_number)
    .bind(seed.track_number)
    .execute(&services.catalog_replication.store.pool)
    .await
    .unwrap();
}

pub(crate) fn assert_source(actual: &crate::LocalSourceFact, expected: &SourceExpectation) {
    assert_eq!(actual.id, expected.id);
    assert_eq!(actual.observation_id, None);
    assert_eq!(actual.title, expected.title);
    assert_eq!(actual.artist, expected.artist);
    assert_eq!(actual.duration_ms, expected.duration_ms);
    assert_eq!(actual.server_relay, expected.relay);
    let actual_release = actual.release.as_ref().unwrap();
    assert_eq!(actual_release.id, expected.release.id);
    assert_eq!(actual_release.entry_id, expected.id);
    assert_eq!(actual_release.title, expected.release.title);
    assert_eq!(actual_release.artist, expected.release.artist);
    assert_eq!(actual_release.disc_number, expected.release.disc_number);
    assert_eq!(actual_release.track_number, expected.release.track_number);
}

pub(crate) fn assert_recording(
    actual: &crate::LocalRecordingFact,
    expected: &RecordingExpectation,
    library: &str,
) {
    assert_eq!(actual.id, expected.id);
    assert_eq!(actual.library_id, library);
    assert_eq!(actual.title, expected.title);
    assert_eq!(actual.artist, expected.artist);
    assert_eq!(actual.duration_ms, expected.duration_ms);
    assert_eq!(actual.sources.len(), expected.sources.len());
    for (actual_source, expected_source) in actual.sources.iter().zip(&expected.sources) {
        assert_source(actual_source, expected_source);
    }
}

pub(crate) struct Fixture {
    pub(crate) dir: tempfile::TempDir,
    pub(crate) services: crate::Services,
    pub(crate) actor: crate::Actor,
    pub(crate) library: String,
    pub(crate) foreign_actor: crate::Actor,
    pub(crate) foreign_library: String,
    pub(crate) foreign_user_id: String,
    pub(crate) expected: Vec<RecordingExpectation>,
}

impl Fixture {
    pub(crate) async fn seed() -> Self {
        let (dir, services, actor, library, music) =
            support::library("issue146-batch-boundary").await;
        support::write_wav(&music.join("owned.wav"), 0);
        support::scan(&services, &actor, &library).await;

        let baseline_id: String =
            sqlx::query_scalar("SELECT id FROM recordings WHERE library_id=? ORDER BY id LIMIT 1")
                .bind(&library)
                .fetch_one(&services.catalog_replication.store.pool)
                .await
                .unwrap();
        let baseline_source_id: String = sqlx::query_scalar(
            "SELECT id FROM sources WHERE recording_id=? AND provider='local' ORDER BY id LIMIT 1",
        )
        .bind(&baseline_id)
        .fetch_one(&services.catalog_replication.store.pool)
        .await
        .unwrap();

        for number in 0..3 {
            sqlx::query(
                "INSERT INTO releases(id,library_id,title,artist,identity_key) VALUES (?,?,?,?,?)",
            )
            .bind(format!("boundary-release-{number}"))
            .bind(&library)
            .bind(format!("Release {number}"))
            .bind(if number == 1 {
                None
            } else {
                Some(format!("Release Artist {number}"))
            })
            .bind(format!("issue146-boundary-release-{number}"))
            .execute(&services.catalog_replication.store.pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO releases(id,library_id,title,artist,identity_key) VALUES (?,?,?,?,?)",
        )
        .bind("boundary-release-owned")
        .bind(&library)
        .bind("Owned release")
        .bind("Owned release artist")
        .bind("issue146-boundary-release-owned")
        .execute(&services.catalog_replication.store.pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE recordings SET release_id=?,title='Boundary 999',artist='Owned Artist',duration_ms=123456
             WHERE id=? AND library_id=?",
        )
        .bind("boundary-release-owned")
        .bind(&baseline_id)
        .bind(&library)
        .execute(&services.catalog_replication.store.pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE source_metadata SET observation_id=NULL,release_id=?,title='Owned source title',
             artist='Owned source artist',duration_ms=65432,disc_number=2,track_number=9 WHERE source_id=?",
        )
        .bind("boundary-release-owned")
        .bind(&baseline_source_id)
        .execute(&services.catalog_replication.store.pool)
        .await
        .unwrap();

        sqlx::query(
            "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i+1<512)
             INSERT INTO recordings(id,library_id,release_id,title,artist,duration_ms)
             SELECT 'boundary-recording-'||printf('%03d',i),?,'boundary-release-'||(i%3),
                    'Boundary '||printf('%03d',i/2),'Recording Artist '||printf('%02d',i%11),
                    60000+i*17 FROM n",
        )
        .bind(&library)
        .execute(&services.catalog_replication.store.pool)
        .await
        .unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i+1<512)
             INSERT INTO sources(id,recording_id,provider,provider_id,ready)
             SELECT 'boundary-source-'||printf('%03d',i)||'-a',
                    'boundary-recording-'||printf('%03d',i),'local',
                    'boundary-provider-'||printf('%03d',i)||'-a',0 FROM n",
        )
        .execute(&services.catalog_replication.store.pool)
        .await
        .unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i+1<512)
             INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,disc_number,track_number)
             SELECT 'boundary-source-'||printf('%03d',i)||'-a','boundary-release-'||(i%3),
                    'Source title '||printf('%03d',i),'Source Artist '||printf('%02d',i%7),
                    40000+i*13,(i%3)+1,(i%17)+7 FROM n",
        )
        .execute(&services.catalog_replication.store.pool)
        .await
        .unwrap();

        for number in [255, 256] {
            let recording_id = format!("boundary-recording-{number:03}");
            let source_id = format!("boundary-source-{number:03}-b");
            let release_number = (number + 1) % 3;
            insert_source(
                &services,
                SourceSeed {
                    recording_id,
                    source_id,
                    device_id: Some("another-device".into()),
                    ready: true,
                    release_id: format!("boundary-release-{release_number}"),
                    title: format!("Secondary source {number}"),
                    artist: format!("Secondary artist {number}"),
                    duration_ms: 90000 + number as i64,
                    disc_number: None,
                    track_number: Some(4),
                },
            )
            .await;
        }
        let baseline_alternate = "boundary-source-owned-other-device";
        insert_source(
            &services,
            SourceSeed {
                recording_id: baseline_id.clone(),
                source_id: baseline_alternate.into(),
                device_id: Some("another-device".into()),
                ready: true,
                release_id: "boundary-release-2".into(),
                title: "Owned alternate source".into(),
                artist: "Owned alternate artist".into(),
                duration_ms: 76543,
                disc_number: Some(1),
                track_number: None,
            },
        )
        .await;

        let foreign_user = services
            .identity
            .create_user(
                &actor,
                Credentials {
                    username: "issue146-foreign-account".into(),
                    password: "fixture-only account password".into(),
                },
            )
            .await
            .unwrap();
        let foreign_library = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO libraries(id,name) VALUES (?,?)")
            .bind(&foreign_library)
            .bind("Foreign fixture library")
            .execute(&services.catalog_replication.store.pool)
            .await
            .unwrap();
        services
            .identity
            .set_library_access(&actor, &foreign_library, &foreign_user.id, true)
            .await
            .unwrap();
        let foreign_actor = services
            .identity
            .login(Credentials {
                username: "issue146-foreign-account".into(),
                password: "fixture-only account password".into(),
            })
            .await
            .unwrap();
        let foreign_actor = services
            .identity
            .authenticate(&foreign_actor.token)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO recordings(id,library_id,title,artist,duration_ms)
             VALUES ('foreign-boundary-recording',?,'PRIVATE-CROSS-LIBRARY-SENTINEL',
                     'Private foreign artist',77777)",
        )
        .bind(&foreign_library)
        .execute(&services.catalog_replication.store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sources(id,recording_id,provider,provider_id,ready)
             VALUES ('foreign-boundary-source','foreign-boundary-recording','local',
                     'foreign-boundary-provider',0)",
        )
        .execute(&services.catalog_replication.store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO source_metadata(source_id,title,artist,duration_ms)
             VALUES ('foreign-boundary-source','Private foreign source','Private source artist',88888)",
        )
        .execute(&services.catalog_replication.store.pool)
        .await
        .unwrap();

        let mut expected = (0..512)
            .map(|number| {
                let mut sources = vec![SourceExpectation {
                    id: format!("boundary-source-{number:03}-a"),
                    title: format!("Source title {number:03}"),
                    artist: format!("Source Artist {:02}", number % 7),
                    duration_ms: 40000 + number as i64 * 13,
                    relay: ServerRelayObservation::Unavailable,
                    release: release(number % 3),
                }];
                if [255, 256].contains(&number) {
                    let release_number = (number + 1) % 3;
                    sources.push(SourceExpectation {
                        id: format!("boundary-source-{number:03}-b"),
                        title: format!("Secondary source {number}"),
                        artist: format!("Secondary artist {number}"),
                        duration_ms: 90000 + number as i64,
                        relay: ServerRelayObservation::Unavailable,
                        release: ReleaseExpectation {
                            disc_number: None,
                            track_number: Some(4),
                            ..release(release_number)
                        },
                    });
                }
                sources.sort_by(|left, right| left.id.cmp(&right.id));
                RecordingExpectation {
                    id: format!("boundary-recording-{number:03}"),
                    title: format!("Boundary {:03}", number / 2),
                    artist: format!("Recording Artist {:02}", number % 11),
                    duration_ms: 60000 + number as i64 * 17,
                    sources,
                }
            })
            .collect::<Vec<_>>();
        let mut baseline_sources = vec![SourceExpectation {
            id: baseline_source_id,
            title: "Owned source title".into(),
            artist: "Owned source artist".into(),
            duration_ms: 65432,
            relay: ServerRelayObservation::Ready,
            release: ReleaseExpectation {
                id: "boundary-release-owned".into(),
                title: "Owned release".into(),
                artist: Some("Owned release artist".into()),
                disc_number: Some(2),
                track_number: Some(9),
            },
        }];
        baseline_sources.push(SourceExpectation {
            id: baseline_alternate.into(),
            title: "Owned alternate source".into(),
            artist: "Owned alternate artist".into(),
            duration_ms: 76543,
            relay: ServerRelayObservation::Unavailable,
            release: ReleaseExpectation {
                disc_number: Some(1),
                track_number: None,
                ..release(2)
            },
        });
        baseline_sources.sort_by(|left, right| left.id.cmp(&right.id));
        expected.push(RecordingExpectation {
            id: baseline_id,
            title: "Boundary 999".into(),
            artist: "Owned Artist".into(),
            duration_ms: 123456,
            sources: baseline_sources,
        });
        expected.sort_by(|left, right| (&left.title, &left.id).cmp(&(&right.title, &right.id)));

        Self {
            dir,
            services,
            actor,
            library,
            foreign_actor,
            foreign_library,
            foreign_user_id: foreign_user.id,
            expected,
        }
    }
}
