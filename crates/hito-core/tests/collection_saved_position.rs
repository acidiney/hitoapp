#[path = "fixtures/playback.rs"]
mod fixture;
mod support;

use hito_core::Action;
use uuid::Uuid;

#[tokio::test]
async fn saved_position_distinguishes_missing_zero_and_current_progress() {
    let f = fixture::LocalPlayback::start().await;
    let recording = Uuid::parse_str(&f.recording).unwrap();
    assert_eq!(
        f.services
            .collection
            .saved_position(&f.actor, recording)
            .await
            .unwrap(),
        None
    );
    for position_ms in [0, 700, 50] {
        support::synchronize(
            &f.services,
            &f.actor,
            vec![Action::ProgressSet {
                recording_id: recording,
                position_ms,
                speed: 2.5,
            }],
        )
        .await;
        let saved = f
            .services
            .collection
            .saved_position(&f.actor, recording)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.position_ms, position_ms as u64);
    }
}

#[tokio::test]
async fn saved_position_requires_current_membership_and_a_live_retained_session() {
    let f = fixture::LocalPlayback::start().await;
    let recording = Uuid::parse_str(&f.recording).unwrap();
    support::synchronize(
        &f.services,
        &f.actor,
        vec![Action::ProgressSet {
            recording_id: recording,
            position_ms: 700,
            speed: 1.5,
        }],
    )
    .await;
    let credentials = || hito_core::Credentials {
        username: "saved-listener".into(),
        password: "private saved position password".into(),
    };
    let user = f
        .services
        .identity
        .create_user(&f.actor, credentials())
        .await
        .unwrap();
    let session = f.services.identity.login(credentials()).await.unwrap();
    let listener = f
        .services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .collection
            .saved_position(&listener, recording)
            .await,
        Err(hito_core::Error::NotFound)
    ));
    f.services
        .identity
        .set_library_access(&f.actor, &f.library, &user.id, true)
        .await
        .unwrap();
    assert_eq!(
        f.services
            .collection
            .saved_position(&listener, recording)
            .await
            .unwrap(),
        None
    );
    support::synchronize(
        &f.services,
        &listener,
        vec![Action::ProgressSet {
            recording_id: recording,
            position_ms: 300,
            speed: 0.5,
        }],
    )
    .await;
    assert_eq!(
        f.services
            .collection
            .saved_position(&f.actor, recording)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        700
    );
    assert_eq!(
        f.services
            .collection
            .saved_position(&listener, recording)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        300
    );
    f.services
        .identity
        .set_library_access(&f.actor, &f.library, &user.id, false)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .collection
            .saved_position(&listener, recording)
            .await,
        Err(hito_core::Error::NotFound)
    ));
    f.services.identity.logout(&f.token).await.unwrap();
    assert!(matches!(
        f.services
            .collection
            .saved_position(&f.actor, recording)
            .await,
        Err(hito_core::Error::Unauthorized)
    ));
}

#[tokio::test]
async fn saved_position_is_independent_of_the_recent_history_window() {
    let (_directory, services, actor, library, music) = support::library("saved-window").await;
    for index in 0..205 {
        support::write_wav(&music.join(format!("{index}.wav")), index);
    }
    support::scan(&services, &actor, &library).await;
    let search = hito_core::Search {
        limit: Some(200),
        ..Default::default()
    };
    let mut recordings = services
        .catalog
        .recordings(&actor, search)
        .await
        .unwrap()
        .items;
    let tail = services
        .catalog
        .recordings(
            &actor,
            hito_core::Search {
                offset: Some(200),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    recordings.extend(tail.items);
    assert_eq!(recordings.len(), 205);
    for batch in recordings.chunks(100) {
        support::synchronize(
            &services,
            &actor,
            batch
                .iter()
                .map(|r| Action::ProgressSet {
                    recording_id: Uuid::parse_str(&r.id).unwrap(),
                    position_ms: 123,
                    speed: 1.,
                })
                .collect(),
        )
        .await;
    }
    let history = services.collection.activity(&actor).await.unwrap().history;
    assert_eq!(history.len(), 200);
    let older = recordings
        .iter()
        .find(|r| !history.iter().any(|h| h.id == r.id))
        .unwrap();
    assert_eq!(
        services
            .collection
            .saved_position(&actor, Uuid::parse_str(&older.id).unwrap())
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        123
    );
    assert!(matches!(
        services
            .collection
            .saved_position(&actor, Uuid::new_v4())
            .await,
        Err(hito_core::Error::NotFound)
    ));
}
