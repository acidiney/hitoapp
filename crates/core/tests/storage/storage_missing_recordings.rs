use hito_core::test_support::support;
use hito_core::{Action, Actor, Credentials, Error, MissingRecordingsRemoval, Search, Services};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const WINDOW_DAYS: i64 = 31;

struct World {
    dir: tempfile::TempDir,
    services: Services,
    actor: Actor,
    library: String,
    music: PathBuf,
}

impl World {
    async fn with_tracks(names: &[&str]) -> Self {
        let (dir, services, actor, library, music) = support::library("server").await;
        for (offset, name) in names.iter().enumerate() {
            support::write_wav(&music.join(format!("{name}.wav")), offset as i16);
        }
        support::scan(&services, &actor, &library).await;
        Self {
            dir,
            services,
            actor,
            library,
            music,
        }
    }

    async fn connection(&self) -> SqliteConnection {
        connect(self.dir.path()).await
    }

    async fn recording(&self, title: &str) -> String {
        sqlx::query_scalar("SELECT id FROM recordings WHERE title=?")
            .bind(title)
            .fetch_one(&mut self.connection().await)
            .await
            .unwrap()
    }

    async fn vanish(&self, names: &[&str]) {
        for name in names {
            std::fs::remove_file(self.music.join(format!("{name}.wav"))).unwrap();
        }
        support::scan(&self.services, &self.actor, &self.library).await;
    }

    async fn age(&self, title: &str, days: i64) {
        sqlx::query("UPDATE source_unavailability SET since=unixepoch()-?1*86400 WHERE source_id IN (SELECT id FROM sources WHERE recording_id=(SELECT id FROM recordings WHERE title=?2))")
            .bind(days)
            .bind(title)
            .execute(&mut self.connection().await)
            .await
            .unwrap();
    }

    async fn titles(&self) -> Vec<String> {
        let mut titles: Vec<String> = self
            .services
            .catalog
            .recordings(&self.actor, Search::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|recording| recording.title)
            .collect();
        titles.sort();
        titles
    }
}

async fn connect(directory: &Path) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(directory.join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

async fn count(connection: &mut SqliteConnection, sql: &str, bind: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(connection)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_recording_gone_for_longer_than_the_window_is_purged_and_a_younger_one_is_kept() {
    let world = World::with_tracks(&["old", "young", "present"]).await;
    world.vanish(&["old", "young"]).await;
    world.age("old", WINDOW_DAYS).await;
    world.age("young", 5).await;
    let outcome = world
        .services
        .storage
        .purge_expired_missing_recordings()
        .await
        .unwrap();
    assert_eq!(
        outcome,
        MissingRecordingsRemoval {
            removed: 1,
            kept_referenced: 0,
            more: false
        }
    );
    assert_eq!(world.titles().await, ["present", "young"]);
    let mut connection = world.connection().await;
    for table in [
        "local_files",
        "source_metadata",
        "metadata_observations",
        "recording_evidence",
    ] {
        let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut connection)
            .await
            .unwrap();
        assert_eq!(rows, 2, "{table} keeps only the two other recordings");
    }
    let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM source_unavailability")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(
        waiting, 1,
        "only the younger recording is still counting down"
    );
    let found: i64 =
        sqlx::query_scalar("SELECT count(*) FROM recordings_fts WHERE recordings_fts MATCH 'old'")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(found, 0);
    assert_eq!(
        world
            .services
            .storage
            .purge_expired_missing_recordings()
            .await
            .unwrap(),
        MissingRecordingsRemoval::default()
    );
}

#[tokio::test]
async fn a_recording_an_account_still_references_is_kept_and_reported() {
    let world = World::with_tracks(&["liked", "listed", "playing", "free"]).await;
    let liked = world.recording("liked").await;
    let listed = world.recording("listed").await;
    let playing = world.recording("playing").await;
    let playlist = Uuid::new_v4();
    support::synchronize(
        &world.services,
        &world.actor,
        vec![
            Action::LikeSet {
                recording_id: liked.parse().unwrap(),
                liked: true,
            },
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Kept".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: listed.parse().unwrap(),
                position: 0,
            },
            Action::ProgressSet {
                recording_id: playing.parse().unwrap(),
                position_ms: 400,
                speed: 1.0,
            },
        ],
    )
    .await;
    world.vanish(&["liked", "listed", "playing", "free"]).await;
    for title in ["liked", "listed", "playing", "free"] {
        world.age(title, WINDOW_DAYS).await;
    }
    let outcome = world
        .services
        .storage
        .purge_expired_missing_recordings()
        .await
        .unwrap();
    assert_eq!((outcome.removed, outcome.kept_referenced), (1, 3));
    assert_eq!(world.titles().await, ["liked", "listed", "playing"]);
    let immediate = world
        .services
        .storage
        .remove_missing_recordings(&world.actor, &world.library)
        .await
        .unwrap();
    assert_eq!((immediate.removed, immediate.kept_referenced), (0, 3));
}

#[tokio::test]
async fn purging_erases_every_row_naming_the_recording_and_leaves_other_recordings_untouched() {
    let world = World::with_tracks(&["gone", "kept"]).await;
    let gone = world.recording("gone").await;
    let kept = world.recording("kept").await;
    let mut connection = world.connection().await;
    for (recording, mbid) in [
        (&gone, "11111111-1111-4111-8111-111111111111"),
        (&kept, "22222222-2222-4222-8222-222222222222"),
    ] {
        sqlx::query("INSERT INTO recording_musicbrainz_observations(observation_id, entity, mbid, recording_id) SELECT id, 'recording', ?1, recording_id FROM metadata_observations WHERE recording_id=?2")
            .bind(mbid)
            .bind(recording)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    let tables: Vec<String> = sqlx::query_scalar("SELECT m.name FROM sqlite_master m WHERE m.type='table' AND m.name NOT LIKE 'sqlite_%' AND EXISTS(SELECT 1 FROM pragma_table_info(m.name) WHERE name='recording_id')")
        .fetch_all(&mut connection)
        .await
        .unwrap();
    assert!(tables.len() > 20);
    let mut before = Vec::new();
    for table in &tables {
        before.push(
            count(
                &mut connection,
                &format!("SELECT count(*) FROM {table} WHERE recording_id=?"),
                &kept,
            )
            .await,
        );
    }
    assert!(before.iter().sum::<i64>() >= 4);
    drop(connection);
    world.vanish(&["gone"]).await;
    world.age("gone", WINDOW_DAYS).await;
    let outcome = world
        .services
        .storage
        .purge_expired_missing_recordings()
        .await
        .unwrap();
    assert_eq!(outcome.removed, 1);
    let mut connection = world.connection().await;
    for (index, table) in tables.iter().enumerate() {
        let sql = format!("SELECT count(*) FROM {table} WHERE recording_id=?");
        assert_eq!(count(&mut connection, &sql, &gone).await, 0, "{table}");
        assert_eq!(
            count(&mut connection, &sql, &kept).await,
            before[index],
            "{table}"
        );
    }
    let scopes: i64 = sqlx::query_scalar("SELECT count(*) FROM correction_erasure_scopes")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(scopes, 0);
    let integrity: Vec<(String, i64, String, i64)> = sqlx::query_as("PRAGMA foreign_key_check")
        .fetch_all(&mut connection)
        .await
        .unwrap();
    assert!(integrity.is_empty());
}

#[tokio::test]
async fn a_file_that_returns_or_a_remaining_source_keeps_the_recording() {
    let world = World::with_tracks(&["returns", "twin"]).await;
    std::fs::create_dir(world.music.join("copy")).unwrap();
    std::fs::copy(
        world.music.join("twin.wav"),
        world.music.join("copy/twin.wav"),
    )
    .unwrap();
    support::scan(&world.services, &world.actor, &world.library).await;
    let sources: i64 = sqlx::query_scalar("SELECT count(*) FROM sources WHERE recording_id=(SELECT id FROM recordings WHERE title='twin')")
        .fetch_one(&mut world.connection().await)
        .await
        .unwrap();
    assert_eq!(
        sources, 2,
        "the duplicate file is a second source of one recording"
    );
    world.vanish(&["returns", "twin"]).await;
    world.age("returns", WINDOW_DAYS).await;
    world.age("twin", WINDOW_DAYS).await;
    support::write_wav(&world.music.join("returns.wav"), 0);
    support::scan(&world.services, &world.actor, &world.library).await;
    let outcome = world
        .services
        .storage
        .purge_expired_missing_recordings()
        .await
        .unwrap();
    assert_eq!(outcome.removed, 0);
    assert_eq!(world.titles().await, ["returns", "twin"]);
}

#[tokio::test]
async fn a_recording_with_a_provider_source_is_never_purged() {
    let world = World::with_tracks(&["blended"]).await;
    let recording = world.recording("blended").await;
    sqlx::query("INSERT INTO sources(id, recording_id, provider, provider_id, ready) VALUES ('provider-source', ?, 'plugin', 'remote-1', 0)")
        .bind(&recording)
        .execute(&mut world.connection().await)
        .await
        .unwrap();
    world.vanish(&["blended"]).await;
    sqlx::query("UPDATE source_unavailability SET since=unixepoch()-?*86400")
        .bind(WINDOW_DAYS * 4)
        .execute(&mut world.connection().await)
        .await
        .unwrap();
    let services = &world.services.storage;
    assert_eq!(
        services
            .purge_expired_missing_recordings()
            .await
            .unwrap()
            .removed,
        0
    );
    let immediate = services
        .remove_missing_recordings(&world.actor, &world.library)
        .await
        .unwrap();
    assert_eq!(immediate.removed, 0);
    assert_eq!(world.titles().await, ["blended"]);
}

#[tokio::test]
async fn the_administrator_action_ignores_the_window_but_not_files_that_still_exist() {
    let world = World::with_tracks(&["gone", "present"]).await;
    world.vanish(&["gone"]).await;
    let outcome = world
        .services
        .storage
        .remove_missing_recordings(&world.actor, &world.library)
        .await
        .unwrap();
    assert_eq!((outcome.removed, outcome.kept_referenced), (1, 0));
    assert_eq!(world.titles().await, ["present"]);
    let again = world
        .services
        .storage
        .remove_missing_recordings(&world.actor, &world.library)
        .await
        .unwrap();
    assert_eq!(again, MissingRecordingsRemoval::default());
}

#[tokio::test]
async fn only_an_administrator_with_access_to_the_library_may_remove_missing_recordings() {
    let world = World::with_tracks(&["gone"]).await;
    world.vanish(&["gone"]).await;
    let credentials = || Credentials {
        username: "listener".into(),
        password: "a long listener password".into(),
    };
    world
        .services
        .identity
        .create_user(&world.actor, credentials())
        .await
        .unwrap();
    let session = world.services.identity.login(credentials()).await.unwrap();
    let listener = world
        .services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    world
        .services
        .identity
        .set_library_access(&world.actor, &world.library, &session.user.id, true)
        .await
        .unwrap();
    assert!(matches!(
        world
            .services
            .storage
            .remove_missing_recordings(&listener, &world.library)
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        world
            .services
            .storage
            .remove_missing_recordings(&world.actor, "unknown-library")
            .await,
        Err(Error::NotFound | Error::Forbidden)
    ));
    assert_eq!(world.titles().await, ["gone"]);
}

async fn crowd(world: &World, count: usize) {
    let mut connection = world.connection().await;
    let mut tx = connection.begin().await.unwrap();
    for index in 0..count {
        let id = format!("crowd-{index:05}");
        sqlx::query(
            "INSERT INTO recordings(id, library_id, title, artist) VALUES (?1, ?2, ?1, 'Crowd')",
        )
        .bind(&id)
        .bind(&world.library)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("INSERT INTO sources(id, recording_id, provider, provider_id, device_id, ready) VALUES (?1, ?1, 'local', ?1, 'server', 0)")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE source_unavailability SET since=unixepoch()-?*86400")
        .bind(WINDOW_DAYS)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn crowd_left(world: &World) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM recordings WHERE id LIKE 'crowd-%'")
        .fetch_one(&mut world.connection().await)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_pass_is_bounded_and_the_rest_is_finished_by_later_passes_after_a_restart() {
    let world = World::with_tracks(&["present"]).await;
    crowd(&world, 1200).await;
    let first = world
        .services
        .storage
        .purge_expired_missing_recordings()
        .await
        .unwrap();
    assert!(first.more);
    assert!(first.removed > 0 && first.removed < 1200, "{first:?}");
    assert_eq!(crowd_left(&world).await, 1200 - first.removed);
    let dir = world.dir;
    let services = hito_core::open(hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        dir.path().join("music"),
        hito_core::HostDeviceId::new("server").unwrap(),
    ))
    .await
    .unwrap();
    let mut removed = first.removed;
    let mut passes = 1;
    loop {
        let pass = services
            .storage
            .purge_expired_missing_recordings()
            .await
            .unwrap();
        removed += pass.removed;
        passes += 1;
        assert!(passes < 10);
        if !pass.more {
            break;
        }
    }
    assert_eq!(removed, 1200);
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM recordings")
        .fetch_one(&mut connect(dir.path()).await)
        .await
        .unwrap();
    assert_eq!(remaining, 1);
}

#[tokio::test]
async fn a_failing_recording_rolls_back_whole_while_the_rest_of_the_batch_is_removed() {
    let world = World::with_tracks(&["present"]).await;
    crowd(&world, 5).await;
    let mut connection = world.connection().await;
    sqlx::query("CREATE TRIGGER fail_one BEFORE DELETE ON recordings WHEN old.id='crowd-00004' BEGIN SELECT RAISE(ABORT,'injected failure'); END")
        .execute(&mut connection)
        .await
        .unwrap();
    drop(connection);
    assert!(
        world
            .services
            .storage
            .purge_expired_missing_recordings()
            .await
            .is_err()
    );
    assert_eq!(
        crowd_left(&world).await,
        1,
        "only the failing recording stays"
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM sources WHERE id='crowd-00004'")
        .fetch_one(&mut world.connection().await)
        .await
        .unwrap();
    assert_eq!(
        rows, 1,
        "the failing recording lost nothing before it failed"
    );
    let scopes: i64 = sqlx::query_scalar("SELECT count(*) FROM correction_erasure_scopes")
        .fetch_one(&mut world.connection().await)
        .await
        .unwrap();
    assert_eq!(scopes, 0);
}

async fn age_all(world: &World) {
    sqlx::query("UPDATE source_unavailability SET since=unixepoch()-40*86400")
        .execute(&mut world.connection().await)
        .await
        .unwrap();
}

async fn purge(world: &World) -> i64 {
    world
        .services
        .storage
        .purge_expired_missing_recordings()
        .await
        .unwrap()
        .removed
}

#[tokio::test]
async fn an_unavailable_or_rescanning_folder_never_runs_the_window() {
    let world = World::with_tracks(&["gone"]).await;
    std::fs::remove_file(world.music.join("gone.wav")).unwrap();
    let away = world.dir.path().join("music-away");
    std::fs::rename(&world.music, &away).unwrap();
    let job = world
        .services
        .storage
        .enqueue_scan(&world.actor, &world.library)
        .await
        .unwrap()
        .remove(0);
    assert!(world.services.storage.run_next_job().await.unwrap());
    assert_eq!(
        world
            .services
            .storage
            .job(&world.actor, &job.id)
            .await
            .unwrap()
            .state,
        "failed"
    );
    age_all(&world).await;
    assert_eq!(
        purge(&world).await,
        0,
        "a drive that is away is not a missing file"
    );
    assert!(matches!(
        world
            .services
            .storage
            .remove_missing_recordings(&world.actor, &world.library)
            .await,
        Err(Error::Conflict(_))
    ));
    std::fs::rename(&away, &world.music).unwrap();
    world
        .services
        .storage
        .enqueue_scan(&world.actor, &world.library)
        .await
        .unwrap();
    age_all(&world).await;
    assert_eq!(
        purge(&world).await,
        0,
        "a queued scan has not confirmed the file is gone"
    );
    assert!(world.services.storage.run_next_job().await.unwrap());
    age_all(&world).await;
    assert_eq!(purge(&world).await, 1, "a completed scan confirms it");
}

#[tokio::test]
async fn recordings_of_a_disconnected_folder_are_skipped_by_both_removals() {
    let world = World::with_tracks(&["gone"]).await;
    std::fs::remove_file(world.music.join("gone.wav")).unwrap();
    support::scan(&world.services, &world.actor, &world.library).await;
    let folder = world
        .services
        .storage
        .folders(&world.actor, &world.library)
        .await
        .unwrap()
        .remove(0);
    world
        .services
        .storage
        .remove_folder(&world.actor, &folder.id, folder.revision)
        .await
        .unwrap();
    age_all(&world).await;
    assert_eq!(purge(&world).await, 0);
    let outcome = world
        .services
        .storage
        .remove_missing_recordings(&world.actor, &world.library)
        .await
        .unwrap();
    assert_eq!(outcome.removed, 0);
    assert_eq!(world.titles().await, ["gone"]);
}

#[tokio::test]
async fn a_personal_title_edit_keeps_the_recording() {
    let world = World::with_tracks(&["edited", "plain"]).await;
    let edited = world.recording("edited").await;
    support::synchronize(
        &world.services,
        &world.actor,
        vec![Action::RecordingTitleSet {
            recording_id: edited.parse().unwrap(),
            title: "My name for it".into(),
        }],
    )
    .await;
    world.vanish(&["edited", "plain"]).await;
    age_all(&world).await;
    let outcome = world
        .services
        .storage
        .purge_expired_missing_recordings()
        .await
        .unwrap();
    assert_eq!((outcome.removed, outcome.kept_referenced), (1, 1));
}
