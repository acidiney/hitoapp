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
