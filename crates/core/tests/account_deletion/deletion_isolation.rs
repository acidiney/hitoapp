use super::fixture::{World, connect, mentions, rows_of};
use hito_core::{DeleteAccount, Error};
use std::future::pending;

#[tokio::test]
async fn deleting_an_account_removes_everything_it_owned_and_nothing_of_another_account() {
    let mut world = World::empty().await;
    let kept = world.populate(&world.admin).await;
    let erased = world.populate(&world.member).await;
    let mut connection = connect(world.directory.path()).await;
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
