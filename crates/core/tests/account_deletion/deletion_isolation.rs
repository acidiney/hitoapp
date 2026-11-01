use super::fixture::{World, connect, mentions, rows_of};
use hito_core::{DeleteAccount, Error};
use std::future::pending;

#[tokio::test]
async fn deleting_an_account_removes_everything_it_owned_and_nothing_of_another_account() {
    let mut world = World::empty().await;
    let kept = world.populate(&world.admin).await;
    let erased = world.populate(&world.member).await;
    let mut connection = connect(world.directory.path()).await;
    const KEPT_ARTIST: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const ERASED_ARTIST: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    const UNUSED_ARTIST: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    for (account, artist) in [
        (&world.admin.user.id, KEPT_ARTIST),
        (&world.member.user.id, ERASED_ARTIST),
    ] {
        sqlx::query("INSERT INTO account_artist_identity_groups(id, user_id, provider_kind, provider_artist_id) VALUES (?, ?, 'builtin_musicbrainz', ?)")
            .bind(format!("group-{artist}"))
            .bind(account)
            .bind(artist)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    for artist in [KEPT_ARTIST, ERASED_ARTIST, UNUSED_ARTIST] {
        sqlx::query("INSERT INTO musicbrainz_genre_lookups(entity, mbid, fetched_at) VALUES ('artist', ?1, unixepoch())")
            .bind(artist)
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("INSERT INTO musicbrainz_genres(entity, mbid, genre_key, label) VALUES ('artist', ?1, 'fado', 'Fado')")
            .bind(artist)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    for (account, recording) in [
        (&world.admin.user.id, kept.recording),
        (&world.member.user.id, erased.recording),
    ] {
        let (low, high) = if recording < world.shared_recording {
            (recording, world.shared_recording)
        } else {
            (world.shared_recording, recording)
        };
        sqlx::query("INSERT INTO recording_group_members(user_id, recording_id, canonical_id, proof) VALUES (?1, ?2, ?3, 'isrc')")
            .bind(account)
            .bind(recording.to_string())
            .bind(world.shared_recording.to_string())
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("INSERT INTO recording_group_splits(user_id, recording_id, other_id) VALUES (?1, ?2, ?3)")
            .bind(account)
            .bind(low.to_string())
            .bind(high.to_string())
            .execute(&mut connection)
            .await
            .unwrap();
    }
    let admin_before = rows_of(&mut connection, &world.admin.user.id).await;
    let member_before = rows_of(&mut connection, &world.member.user.id).await;
    assert!(member_before.iter().map(|(_, n)| n).sum::<i64>() > 20);
    assert!(world.file_exists(&kept.files[0]) && world.file_exists(&erased.files[0]));
    let admin = world.actor(&world.admin).await;
    for recording in [erased.recording, kept.recording, world.shared_recording] {
        sqlx::query(
            "INSERT INTO source_genres(source_id, genre_key, label)
             SELECT id, 'fado', 'Fado' FROM sources WHERE recording_id = ?",
        )
        .bind(recording.to_string())
        .execute(&mut connection)
        .await
        .unwrap();
    }
    for recording in [erased.recording, kept.recording] {
        let id = format!("credited-{recording}");
        for statement in [
            "INSERT INTO metadata_observations(id,recording_id,provider,origin,title,artist,duration_ms) VALUES(?1,?2,'owned','provider_response','T','A & B',1000)",
            "INSERT INTO recording_credit_observations(observation_id,recording_id,credit_label) VALUES(?1,?2,'A & B')",
            "INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role) VALUES(?1,0,?2,'A','main')",
        ] {
            sqlx::query(statement)
                .bind(&id)
                .bind(recording.to_string())
                .execute(&mut connection)
                .await
                .unwrap();
        }
    }
    let genres_before: i64 = sqlx::query_scalar("SELECT count(*) FROM source_genres")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert!(genres_before >= 3);

    let outcome = world
        .hito
        .identity
        .delete_account(
            &admin,
            &world.member.user.id,
            DeleteAccount {
                username: "erasable-person".into(),
            },
        )
        .await
        .unwrap();
    assert!(outcome.rows_removed > 20);
    assert_eq!(outcome.files_queued, 1);

    let member_after = rows_of(&mut connection, &world.member.user.id).await;
    assert!(
        member_after.iter().all(|(_, n)| *n == 0),
        "no row may name the erased account: {member_after:?}"
    );
    assert_eq!(
        mentions(&mut connection, &world.member.user.id).await,
        Vec::<String>::new(),
        "no text anywhere may still mention the erased account"
    );
    assert_eq!(
        rows_of(&mut connection, &world.admin.user.id).await,
        admin_before
    );
    let foreign: Vec<(String,)> = sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check")
        .fetch_all(&mut connection)
        .await
        .unwrap();
    assert!(foreign.is_empty(), "foreign keys must hold: {foreign:?}");
    for table in [
        "recording_credit_observations",
        "recording_credited_artist_observations",
    ] {
        let remaining: Vec<String> =
            sqlx::query_scalar(&format!("SELECT recording_id FROM {table} ORDER BY 1"))
                .fetch_all(&mut connection)
                .await
                .unwrap();
        assert_eq!(
            remaining,
            [kept.recording.to_string()],
            "{table}: the erased account's recording takes its structured credits with it"
        );
    }
    let erased_genres: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM source_genres g JOIN sources s ON s.id = g.source_id WHERE s.recording_id = ?",
    )
    .bind(erased.recording.to_string())
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        erased_genres, 0,
        "the erased account's source genres go with its sources"
    );
    let genres_after: i64 = sqlx::query_scalar("SELECT count(*) FROM source_genres")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert!(
        genres_after >= 2 && genres_after < genres_before,
        "the kept and shared sources keep their genres"
    );
    let cached: Vec<String> =
        sqlx::query_scalar("SELECT mbid FROM musicbrainz_genre_lookups ORDER BY mbid")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        cached,
        [KEPT_ARTIST],
        "cached genres stay only for identities a remaining account names"
    );
    let cached_genres: i64 = sqlx::query_scalar("SELECT count(*) FROM musicbrainz_genres")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(cached_genres, 1);
    let scopes: i64 = sqlx::query_scalar("SELECT count(*) FROM correction_erasure_scopes")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(scopes, 0);

    let state = world
        .hito
        .catalog
        .recordings_by_ids(&admin, &[world.shared_recording, kept.recording])
        .await
        .unwrap();
    assert_eq!(
        state.len(),
        2,
        "the kept account still reads shared and own recordings"
    );
    assert!(
        world
            .hito
            .identity
            .authenticate(&world.member.token)
            .await
            .is_err()
    );

    assert!(
        world.file_exists(&erased.files[0]),
        "the file leaves only after commit"
    );
    let directory = world.directory.path().to_owned();
    let origin = world.server.origin.clone();
    drop(admin);
    world.hito = hito_core::open(super::fixture::config(&directory, &origin))
        .await
        .unwrap();
    assert!(
        world
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    assert!(
        !world.file_exists(&erased.files[0]),
        "the erased account's file is removed after a restart"
    );
    assert!(
        world.file_exists(&kept.files[0]),
        "another account's copy of the same episode stays"
    );
    let ledger: i64 = sqlx::query_scalar("SELECT count(*) FROM erased_download_files")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(ledger, 0);
    let admin = world.actor(&world.admin).await;
    let status = world
        .hito
        .downloads
        .status(&admin, &kept.recording.to_string())
        .await
        .unwrap();
    assert!(matches!(status.status, hito_core::DownloadState::Ready));
    assert!(matches!(
        world
            .hito
            .downloads
            .status(&admin, &erased.recording.to_string())
            .await,
        Err(Error::NotFound)
    ));
    assert!(world.server.finish().await);
}

#[tokio::test]
async fn an_erased_file_that_cannot_be_removed_does_not_block_the_others() {
    let world = World::empty().await;
    let downloads = world.directory.path().join("hito.downloads");
    let blocked = (
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-0000000000a1",
    );
    let removable = (
        "ffffffff-0000-4000-8000-000000000002",
        "ffffffff-0000-4000-8000-0000000000a2",
    );
    let stuck = downloads.join(format!("{}.{}.media", blocked.0, blocked.1));
    std::fs::create_dir_all(&stuck).unwrap();
    std::fs::write(stuck.join("keeps-it-a-directory"), b"x").unwrap();
    let loose = downloads.join(format!("{}.{}.media", removable.0, removable.1));
    std::fs::write(&loose, b"audio").unwrap();
    let mut connection = connect(world.directory.path()).await;
    for (file, attempt) in [blocked, removable] {
        sqlx::query("INSERT INTO erased_download_files(file_id,attempt_id,host_device) VALUES(?,?,'account-deletion')")
            .bind(file)
            .bind(attempt)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    world
        .hito
        .downloads
        .run_next_job_until(pending())
        .await
        .unwrap();
    assert!(
        !loose.exists(),
        "a removable file is removed despite an earlier failing entry"
    );
    assert!(stuck.exists());
    let left: Vec<String> = sqlx::query_scalar("SELECT file_id FROM erased_download_files")
        .fetch_all(&mut connection)
        .await
        .unwrap();
    assert_eq!(
        left,
        vec![blocked.0.to_string()],
        "a ledger row is kept until its file is removed"
    );
}
