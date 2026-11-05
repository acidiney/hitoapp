use hito_core::test_support::support;

use hito_core::{Action, Actor, Credentials, Search, Services};
use support::{catalog, synchronize};
use uuid::Uuid;

fn progress(recording: &str, position_ms: i64) -> Action {
    Action::ProgressSet {
        recording_id: Uuid::parse_str(recording).unwrap(),
        position_ms,
        speed: 1.0,
    }
}

async fn recordings(hito: &Services, actor: &Actor) -> Vec<String> {
    hito.catalog
        .recordings(actor, Search::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|recording| recording.id)
        .collect()
}

async fn history(hito: &Services, actor: &Actor) -> Vec<String> {
    hito.collection
        .activity(actor)
        .await
        .unwrap()
        .history
        .into_iter()
        .map(|entry| entry.id)
        .collect()
}

async fn second_account(hito: &Services, admin: &Actor, library: &str) -> Actor {
    let credentials = || Credentials {
        username: "guest".into(),
        password: "history guest password".into(),
    };
    let guest = hito
        .identity
        .create_user(admin, credentials())
        .await
        .unwrap();
    hito.identity
        .set_library_access(admin, library, &guest.id, true)
        .await
        .unwrap();
    let session = hito.identity.login(credentials()).await.unwrap();
    hito.identity.authenticate(&session.token).await.unwrap()
}

async fn saved(hito: &Services, actor: &Actor, recording: &str) -> Option<u64> {
    hito.collection
        .saved_position(actor, Uuid::parse_str(recording).unwrap())
        .await
        .unwrap()
        .map(|saved| saved.position_ms)
}

#[tokio::test]
async fn history_is_recorded_by_default_and_the_preference_is_per_account() {
    let (_dir, hito, admin, library) = catalog().await;
    let guest = second_account(&hito, &admin, &library).await;
    assert!(
        hito.collection
            .history_preference(&admin)
            .await
            .unwrap()
            .enabled
    );
    assert!(
        hito.collection
            .history_preference(&guest)
            .await
            .unwrap()
            .enabled
    );
    let preference = hito
        .collection
        .set_history_enabled(&admin, false)
        .await
        .unwrap();
    assert!(!preference.enabled);
    assert!(
        !hito
            .collection
            .history_preference(&admin)
            .await
            .unwrap()
            .enabled
    );
    assert!(
        hito.collection
            .history_preference(&guest)
            .await
            .unwrap()
            .enabled
    );
    assert!(
        hito.collection
            .set_history_enabled(&admin, true)
            .await
            .unwrap()
            .enabled
    );
    assert!(
        hito.collection
            .history_preference(&admin)
            .await
            .unwrap()
            .enabled
    );
}

#[tokio::test]
async fn pausing_history_stops_new_entries_but_keeps_resume_positions_and_old_entries() {
    let (_dir, hito, actor, _library) = catalog().await;
    let ids = recordings(&hito, &actor).await;
    synchronize(&hito, &actor, vec![progress(&ids[0], 100)]).await;
    assert_eq!(history(&hito, &actor).await, vec![ids[0].clone()]);

    hito.collection
        .set_history_enabled(&actor, false)
        .await
        .unwrap();
    synchronize(
        &hito,
        &actor,
        vec![progress(&ids[1], 250), progress(&ids[0], 400)],
    )
    .await;
    assert_eq!(history(&hito, &actor).await, vec![ids[0].clone()]);
    assert_eq!(saved(&hito, &actor, &ids[1]).await, Some(250));
    assert_eq!(saved(&hito, &actor, &ids[0]).await, Some(400));
    let entry = &hito.collection.activity(&actor).await.unwrap().history[0];
    assert_eq!(entry.position_ms, 400);

    hito.collection
        .set_history_enabled(&actor, true)
        .await
        .unwrap();
    synchronize(&hito, &actor, vec![progress(&ids[1], 300)]).await;
    let mut recorded = history(&hito, &actor).await;
    recorded.sort();
    let mut expected = vec![ids[0].clone(), ids[1].clone()];
    expected.sort();
    assert_eq!(recorded, expected);
}

#[tokio::test]
async fn deleting_history_removes_only_this_accounts_entries() {
    let (_dir, hito, actor, library) = catalog().await;
    let guest = second_account(&hito, &actor, &library).await;
    let ids = recordings(&hito, &actor).await;
    let playlist = Uuid::new_v4();
    synchronize(
        &hito,
        &actor,
        vec![
            progress(&ids[0], 111),
            progress(&ids[1], 222),
            Action::LikeSet {
                recording_id: Uuid::parse_str(&ids[0]).unwrap(),
                liked: true,
            },
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Kept playlist".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: Uuid::parse_str(&ids[1]).unwrap(),
                position: 0,
            },
        ],
    )
    .await;
    synchronize(&hito, &guest, vec![progress(&ids[2], 333)]).await;
    hito.collection
        .set_history_enabled(&guest, false)
        .await
        .unwrap();

    let cleared = hito.collection.clear_history(&actor).await.unwrap();
    assert_eq!(cleared.removed, 2);
    assert!(history(&hito, &actor).await.is_empty());
    assert_eq!(saved(&hito, &actor, &ids[0]).await, Some(111));
    assert_eq!(saved(&hito, &actor, &ids[1]).await, Some(222));
    let likes = hito
        .catalog
        .liked_recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(likes.items.len(), 1);
    assert_eq!(likes.items[0].id, ids[0]);
    let playlists = hito.collection.playlists(&actor).await.unwrap();
    assert_eq!(playlists.len(), 1);
    assert_eq!(history(&hito, &guest).await, vec![ids[2].clone()]);
    assert!(
        !hito
            .collection
            .history_preference(&guest)
            .await
            .unwrap()
            .enabled
    );
    assert!(
        hito.collection
            .history_preference(&actor)
            .await
            .unwrap()
            .enabled
    );

    assert_eq!(
        hito.collection.clear_history(&actor).await.unwrap().removed,
        0
    );
    synchronize(&hito, &actor, vec![progress(&ids[3], 5)]).await;
    assert_eq!(history(&hito, &actor).await, vec![ids[3].clone()]);
}

#[tokio::test]
async fn recently_played_on_home_follows_history() {
    let (_dir, hito, actor, _library) = catalog().await;
    let ids = recordings(&hito, &actor).await;
    hito.collection
        .set_history_enabled(&actor, false)
        .await
        .unwrap();
    synchronize(&hito, &actor, vec![progress(&ids[0], 100)]).await;
    assert!(
        hito.collection
            .home(&actor)
            .await
            .unwrap()
            .recent_albums
            .is_empty()
    );

    hito.collection
        .set_history_enabled(&actor, true)
        .await
        .unwrap();
    synchronize(&hito, &actor, vec![progress(&ids[0], 100)]).await;
    let home = hito.collection.home(&actor).await.unwrap();
    assert!(!home.recent_albums.is_empty());
    hito.collection.clear_history(&actor).await.unwrap();
    let home = hito.collection.home(&actor).await.unwrap();
    assert!(home.recent_albums.is_empty());
    assert!(home.unheard.is_empty());
    assert_eq!(saved(&hito, &actor, &ids[0]).await, Some(100));
}

#[tokio::test]
async fn a_revoked_session_cannot_read_change_or_clear_history_settings() {
    let (_dir, hito, admin, library) = catalog().await;
    second_account(&hito, &admin, &library).await;
    let session = hito
        .identity
        .login(Credentials {
            username: "guest".into(),
            password: "history guest password".into(),
        })
        .await
        .unwrap();
    let guest = hito.identity.authenticate(&session.token).await.unwrap();
    hito.identity.logout(&session.token).await.unwrap();
    assert!(hito.collection.history_preference(&guest).await.is_err());
    assert!(
        hito.collection
            .set_history_enabled(&guest, false)
            .await
            .is_err()
    );
    assert!(hito.collection.clear_history(&guest).await.is_err());
}
