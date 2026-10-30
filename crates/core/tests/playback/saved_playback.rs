use hito_core::test_support::support;

use hito_core::{Action, Credentials, Error, Search};
use uuid::Uuid;

#[tokio::test]
async fn saved_playback_is_bounded_private_and_independent_of_recent_history() {
    let (_directory, services, owner, library, music) = support::library("saved-playback").await;
    for index in 0..204 {
        support::write_wav(&music.join(format!("{index}.wav")), index);
    }
    support::scan(&services, &owner, &library).await;
    let mut recordings = services
        .catalog
        .recordings(
            &owner,
            Search {
                limit: Some(200),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .items;
    recordings.extend(
        services
            .catalog
            .recordings(
                &owner,
                Search {
                    offset: Some(200),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .items,
    );
    let recordings: Vec<Uuid> = recordings
        .iter()
        .map(|recording| Uuid::parse_str(&recording.id).unwrap())
        .collect();
    assert_eq!(recordings.len(), 204);
    let target = recordings[0];
    let second = recordings[1];
    let unsaved = recordings[2];
    support::synchronize(
        &services,
        &owner,
        vec![Action::ProgressSet {
            recording_id: target,
            position_ms: 700,
            speed: 1.75,
        }],
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    for batch in recordings[3..].chunks(100) {
        support::synchronize(
            &services,
            &owner,
            batch
                .iter()
                .map(|recording_id| Action::ProgressSet {
                    recording_id: *recording_id,
                    position_ms: 123,
                    speed: 1.,
                })
                .collect(),
        )
        .await;
    }
    support::synchronize(
        &services,
        &owner,
        vec![Action::ProgressSet {
            recording_id: second,
            position_ms: 0,
            speed: 0.75,
        }],
    )
    .await;
    let history = services.collection.activity(&owner).await.unwrap().history;
    assert_eq!(history.len(), 200);
    assert!(!history.iter().any(|entry| entry.id == target.to_string()));

    let saved = services
        .collection
        .saved_playback(&owner, &[second, target, second, unsaved, target])
        .await
        .unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0].recording_id, second);
    assert_eq!(saved[0].position_ms, 0);
    assert_eq!(saved[0].speed, 0.75);
    assert_eq!(saved[1].recording_id, target);
    assert_eq!(saved[1].position_ms, 700);
    assert_eq!(saved[1].speed, 1.75);
    assert!(
        services
            .collection
            .saved_playback(&owner, &[])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        services
            .collection
            .saved_playback(&owner, &[target, Uuid::new_v4()])
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        services
            .collection
            .saved_playback(&owner, &vec![target; 201])
            .await,
        Err(Error::Invalid(_))
    ));

    let credentials = || Credentials {
        username: "saved-playback-listener".into(),
        password: "private saved playback password".into(),
    };
    let listener_user = services
        .identity
        .create_user(&owner, credentials())
        .await
        .unwrap();
    let session = services.identity.login(credentials()).await.unwrap();
    let listener = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(matches!(
        services
            .collection
            .saved_playback(&listener, &[target, unsaved])
            .await,
        Err(Error::NotFound)
    ));
    services
        .identity
        .set_library_access(&owner, &library, &listener_user.id, true)
        .await
        .unwrap();
    assert!(
        services
            .collection
            .saved_playback(&listener, &[target, unsaved])
            .await
            .unwrap()
            .is_empty()
    );
    support::synchronize(
        &services,
        &listener,
        vec![Action::ProgressSet {
            recording_id: target,
            position_ms: 300,
            speed: 2.5,
        }],
    )
    .await;
    let listener_saved = services
        .collection
        .saved_playback(&listener, &[target])
        .await
        .unwrap();
    assert_eq!(listener_saved[0].position_ms, 300);
    assert_eq!(listener_saved[0].speed, 2.5);
    let owner_saved = services
        .collection
        .saved_playback(&owner, &[target])
        .await
        .unwrap();
    assert_eq!(owner_saved[0].position_ms, 700);
    assert_eq!(owner_saved[0].speed, 1.75);
    services
        .identity
        .set_library_access(&owner, &library, &listener_user.id, false)
        .await
        .unwrap();
    assert!(matches!(
        services
            .collection
            .saved_playback(&listener, &[unsaved, target])
            .await,
        Err(Error::NotFound)
    ));
    services.identity.logout(&session.token).await.unwrap();
    assert!(matches!(
        services.collection.saved_playback(&listener, &[]).await,
        Err(Error::Unauthorized)
    ));
}
