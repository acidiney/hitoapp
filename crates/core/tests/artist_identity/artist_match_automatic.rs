#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{ArtistAssociationEvidence, ArtistMatchingState, UpdateAutomaticLookups};
use library::{CREDIT, NOVA, NameLibrary, nova_discography};
use std::time::{Duration, Instant};

fn expect_nova(library: &NameLibrary) {
    library.musicbrainz.search(
        CREDIT,
        Reply::Candidates(vec![(NOVA.into(), CREDIT.into())]),
    );
    library.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
            ("Harbor Lights", Some(243_000)),
        ]),
    );
}

#[tokio::test]
async fn an_import_schedules_a_job_that_matches_by_name_and_then_leaves_nothing_behind() {
    let library = NameLibrary::nova().await;
    expect_nova(&library);
    assert_eq!(library.rows("artist_matching_jobs").await, 1);

    let steps = library.automatic().await;
    assert!(steps >= 2, "The walk takes tag, evidence and name steps");

    let group = library.group_of(&library.recordings[0]).await.unwrap();
    for matched in &library.recordings[..3] {
        assert_eq!(library.group_of(matched).await.as_ref(), Some(&group));
        let linked = library.associations(matched).await;
        assert_eq!(
            linked[0].evidence,
            ArtistAssociationEvidence::MatchedNameDiscography
        );
    }
    assert!(library.group_of(&library.recordings[3]).await.is_none());
    assert_eq!(library.rows("artist_matching_jobs").await, 0);
    assert_eq!(
        library.automatic().await,
        0,
        "A finished walk leaves no work"
    );
}

#[tokio::test]
async fn the_walk_persists_its_cursor_between_steps_and_resumes_from_it() {
    let library = NameLibrary::nova().await;
    expect_nova(&library);
    assert!(
        library
            .hito
            .artist_catalog
            .run_next_automatic_matching_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(library.rows("artist_matching_jobs").await, 1);
    assert_eq!(
        library
            .scalar("SELECT phase FROM artist_matching_jobs")
            .await,
        "provider",
        "The tag pass finished and the stored position moved on"
    );
    assert_eq!(
        library
            .scalar("SELECT lease_token IS NULL FROM artist_matching_jobs")
            .await,
        "1",
        "No step keeps the account locked once it ends"
    );
    library.automatic().await;
    assert!(library.group_of(&library.recordings[0]).await.is_some());
}

#[tokio::test]
async fn turning_the_choice_off_stops_the_job_without_any_lookup_and_manual_runs_still_work() {
    let library = NameLibrary::nova().await;
    expect_nova(&library);
    library
        .hito
        .artist_catalog
        .set_automatic_lookups(
            &library.actor,
            UpdateAutomaticLookups {
                automatic_artist_lookup: Some(false),
                automatic_artwork: None,
                automatic_service_search: None,
            },
        )
        .await
        .unwrap();
    library.automatic().await;
    assert!(library.musicbrainz.requests().is_empty());
    assert_eq!(library.rows("artist_matching_jobs").await, 0);
    assert!(library.group_of(&library.recordings[0]).await.is_none());

    let receipt = library.run().await;
    assert_eq!(receipt.matched, 3, "The manual button is unaffected");
}

#[tokio::test]
async fn a_disabled_metadata_policy_skips_the_job() {
    let library = NameLibrary::nova().await;
    expect_nova(&library);
    let settings = library
        .hito
        .artist_catalog
        .metadata_settings(&library.actor)
        .await
        .unwrap();
    library
        .hito
        .artist_catalog
        .set_metadata_settings(
            &library.actor,
            hito_core::UpdateMetadataSettings {
                revision: settings.revision,
                enabled: false,
                providers: vec![],
            },
        )
        .await
        .unwrap();
    library.automatic().await;
    assert!(library.musicbrainz.requests().is_empty());
    assert_eq!(library.rows("artist_matching_jobs").await, 0);
}

#[tokio::test]
async fn shutdown_cancels_a_step_promptly_and_the_job_stays_resumable() {
    let library = NameLibrary::nova().await;
    expect_nova(&library);
    assert!(
        library
            .hito
            .artist_catalog
            .run_next_automatic_matching_until(std::future::pending())
            .await
            .unwrap()
    );
    library.musicbrainz.delay_replies(Duration::from_secs(30));
    let started = Instant::now();
    let stopped = library
        .hito
        .artist_catalog
        .run_next_automatic_matching_until(tokio::time::sleep(Duration::from_millis(300)))
        .await;
    assert!(stopped.is_ok());
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(library.rows("artist_matching_jobs").await, 1);
    assert_eq!(
        library
            .scalar("SELECT lease_token IS NULL FROM artist_matching_jobs")
            .await,
        "1"
    );
    library.musicbrainz.delay_replies(Duration::ZERO);
    library.automatic().await;
    assert!(library.group_of(&library.recordings[0]).await.is_some());
}

#[tokio::test]
async fn a_finished_scan_schedules_one_job_for_every_account_with_access_and_coalesces_repeats() {
    use hito_core::{Credentials, test_support::support};
    use sqlx::Connection;

    let (directory, hito, admin, library, music) = support::library("auto-scan").await;
    let peer = hito
        .identity
        .create_user(
            &admin,
            Credentials {
                username: "peer".into(),
                password: "owned scan peer password".into(),
            },
        )
        .await
        .unwrap();
    hito.identity
        .set_library_access(&admin, &library, &peer.id, true)
        .await
        .unwrap();
    let outsider = hito
        .identity
        .create_user(
            &admin,
            Credentials {
                username: "outsider".into(),
                password: "owned scan outsider password".into(),
            },
        )
        .await
        .unwrap();
    support::write_wav(&music.join("one.wav"), 1);
    support::scan(&hito, &admin, &library).await;
    support::scan(&hito, &admin, &library).await;

    let mut connection = sqlx::SqliteConnection::connect(&format!(
        "sqlite://{}?mode=ro",
        directory.path().join("hito.db").display()
    ))
    .await
    .unwrap();
    let accounts: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM artist_matching_jobs ORDER BY user_id")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    let mut expected = vec![admin.user().id.clone(), peer.id.clone()];
    expected.sort();
    assert_eq!(
        accounts, expected,
        "Only accounts with access, one row each"
    );
    assert!(!accounts.contains(&outsider.id));
}

#[tokio::test]
async fn the_status_reports_waiting_work_the_last_result_and_the_setting() {
    let library = NameLibrary::nova().await;
    expect_nova(&library);
    let artists = &library.hito.artist_catalog;
    let waiting = artists
        .artist_matching_status(&library.actor)
        .await
        .unwrap();
    assert!(waiting.enabled);
    assert_eq!(waiting.state, ArtistMatchingState::Queued);
    assert_eq!(waiting.last_finished_at, None);

    library.automatic().await;
    let done = artists
        .artist_matching_status(&library.actor)
        .await
        .unwrap();
    assert_eq!(done.state, ArtistMatchingState::Idle);
    assert_eq!(done.last_matched, 3);
    assert!(done.last_finished_at.is_some());
    assert_eq!(done.matched_so_far, 0);

    artists
        .set_automatic_lookups(
            &library.actor,
            UpdateAutomaticLookups {
                automatic_artist_lookup: Some(false),
                automatic_artwork: None,
                automatic_service_search: None,
            },
        )
        .await
        .unwrap();
    let off = artists
        .artist_matching_status(&library.actor)
        .await
        .unwrap();
    assert!(!off.enabled);
    assert_eq!(off.state, ArtistMatchingState::Idle);
}
