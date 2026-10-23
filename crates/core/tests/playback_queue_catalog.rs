use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;
use hito_core::{Action, Search};
use uuid::Uuid;

#[tokio::test]
async fn queue_lookup_preserves_order_and_duplicates_with_personal_metadata() {
    let (_directory, hito, actor, _library) = support::catalog().await;
    let tracks = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items;
    let first = Uuid::parse_str(&tracks[4].id).unwrap();
    let second = Uuid::parse_str(&tracks[0].id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::RecordingTitleSet {
            recording_id: first,
            title: "Private queue title".into(),
        }],
    )
    .await;
    let queue = hito
        .catalog
        .recordings_by_ids(&actor, &[first, second, first])
        .await
        .unwrap();
    assert_eq!(
        queue
            .iter()
            .map(|track| track.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            tracks[4].id.as_str(),
            tracks[0].id.as_str(),
            tracks[4].id.as_str()
        ]
    );
    assert_eq!(queue[0].title, "Private queue title");
    assert_eq!(queue[2].title, "Private queue title");
    assert_eq!(queue[0].inherited_title, tracks[4].title);
    assert!(queue.iter().all(|track| track.available));
}

#[tokio::test]
async fn queue_lookup_is_bounded_and_rejects_a_revoked_session_even_for_an_empty_selection() {
    let f = fixture::LocalPlayback::start().await;
    let id = Uuid::parse_str(&f.recording).unwrap();
    assert_eq!(
        f.services
            .catalog
            .recordings_by_ids(&f.actor, &vec![id; 1000])
            .await
            .unwrap()
            .len(),
        1000
    );
    assert!(matches!(
        f.services
            .catalog
            .recordings_by_ids(&f.actor, &vec![id; 1001])
            .await,
        Err(hito_core::Error::Invalid(_))
    ));
    f.services.identity.logout(&f.token).await.unwrap();
    assert!(matches!(
        f.services.catalog.recordings_by_ids(&f.actor, &[]).await,
        Err(hito_core::Error::Unauthorized)
    ));
}

#[tokio::test]
async fn queue_lookup_rejects_missing_or_inaccessible_ids_without_partial_results() {
    let (_directory, hito, actor, library) = support::catalog().await;
    let recording = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let id = Uuid::parse_str(&recording.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::RecordingTitleSet {
            recording_id: id,
            title: "Private owner queue correction".into(),
        }],
    )
    .await;
    assert!(matches!(
        hito.catalog
            .recordings_by_ids(&actor, &[id, Uuid::new_v4()])
            .await,
        Err(hito_core::Error::NotFound)
    ));
    let user = hito
        .identity
        .create_user(
            &actor,
            hito_core::Credentials {
                username: "queue-listener".into(),
                password: "private queue listener password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(hito_core::Credentials {
            username: user.username,
            password: "private queue listener password".into(),
        })
        .await
        .unwrap();
    let listener = hito.identity.authenticate(&session.token).await.unwrap();
    assert!(matches!(
        hito.catalog.recordings_by_ids(&listener, &[id]).await,
        Err(hito_core::Error::NotFound)
    ));
    hito.identity
        .set_library_access(&actor, &library, &user.id, true)
        .await
        .unwrap();
    let visible = hito
        .catalog
        .recordings_by_ids(&listener, &[id])
        .await
        .unwrap();
    assert_eq!(visible[0].id, recording.id);
    assert_eq!(visible[0].title, recording.title);
    hito.identity
        .set_library_access(&actor, &library, &user.id, false)
        .await
        .unwrap();
    assert!(matches!(
        hito.catalog.recordings_by_ids(&listener, &[id]).await,
        Err(hito_core::Error::NotFound)
    ));
}

#[tokio::test]
async fn queue_lookup_retains_permitted_recordings_when_their_local_audio_disappears() {
    let f = fixture::LocalPlayback::start().await;
    std::fs::remove_file(f.music.join("track.wav")).unwrap();
    support::scan(&f.services, &f.actor, &f.library).await;
    let selected = f
        .services
        .catalog
        .recordings_by_ids(&f.actor, &[Uuid::parse_str(&f.recording).unwrap()])
        .await
        .unwrap();
    assert_eq!(selected[0].id, f.recording);
    assert!(!selected[0].available);
}
