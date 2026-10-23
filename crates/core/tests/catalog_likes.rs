use hito_core::test_support::support;

use hito_core::{Action, Credentials, Search};
use support::{catalog, synchronize};
use uuid::Uuid;

#[tokio::test]
async fn paged_recordings_include_only_the_current_users_like_state() {
    let (_dir, hito, actor, library) = catalog().await;
    let bob = hito
        .identity
        .create_user(
            &actor,
            Credentials {
                username: "bob".into(),
                password: "catalog bob password".into(),
            },
        )
        .await
        .unwrap();
    hito.identity
        .set_library_access(&actor, &library, &bob.id, true)
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "bob".into(),
            password: "catalog bob password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    let initial = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(initial.total, 5);
    let liked = &initial.items[1].id;
    synchronize(
        &hito,
        &actor,
        vec![Action::LikeSet {
            recording_id: Uuid::parse_str(liked).unwrap(),
            liked: true,
        }],
    )
    .await;
    synchronize(
        &hito,
        &bob,
        vec![Action::LikeSet {
            recording_id: Uuid::parse_str(&initial.items[0].id).unwrap(),
            liked: true,
        }],
    )
    .await;

    let page = hito
        .catalog
        .recordings(
            &actor,
            Search {
                offset: Some(1),
                limit: Some(2),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 5);
    assert_eq!(page.items.len(), 2);
    let rows = serde_json::to_value(&page.items).unwrap();
    assert_eq!(rows[0]["id"], *liked);
    assert_eq!(rows[0]["liked"], true);
    assert_eq!(rows[1]["liked"], false);
    let bob_rows = serde_json::to_value(
        &hito
            .catalog
            .recordings(&bob, Search::default())
            .await
            .unwrap()
            .items,
    )
    .unwrap();
    assert_eq!(bob_rows[0]["liked"], true);
    assert_eq!(bob_rows[1]["liked"], false);

    let filtered = hito
        .catalog
        .recordings(
            &actor,
            Search {
                liked: Some(true),
                limit: Some(1),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.items[0].id, *liked);
    hito.identity
        .set_library_access(&actor, &library, &bob.user().id, false)
        .await
        .unwrap();
    assert_eq!(
        hito.catalog
            .recordings(&bob, Search::default())
            .await
            .unwrap()
            .total,
        0
    );
}

#[tokio::test]
async fn liked_recordings_list_only_the_callers_likes_whatever_the_search_asks() {
    let (_dir, hito, actor, library) = catalog().await;
    let bob = hito
        .identity
        .create_user(
            &actor,
            Credentials {
                username: "likes-bob".into(),
                password: "catalog likes bob password".into(),
            },
        )
        .await
        .unwrap();
    hito.identity
        .set_library_access(&actor, &library, &bob.id, true)
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "likes-bob".into(),
            password: "catalog likes bob password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    let all = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    let like = |index: usize| Action::LikeSet {
        recording_id: Uuid::parse_str(&all.items[index].id).unwrap(),
        liked: true,
    };
    synchronize(&hito, &actor, vec![like(1), like(3)]).await;
    synchronize(&hito, &bob, vec![like(0)]).await;

    for liked in [None, Some(true), Some(false)] {
        let page = hito
            .catalog
            .liked_recordings(
                &actor,
                Search {
                    liked,
                    ..Search::default()
                },
            )
            .await
            .unwrap();
        let ids: Vec<&str> = page.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(page.total, 2);
        assert_eq!(ids, [all.items[1].id.as_str(), all.items[3].id.as_str()]);
        assert!(page.items.iter().all(|item| item.liked));
    }
    let paged = hito
        .catalog
        .liked_recordings(
            &actor,
            Search {
                offset: Some(1),
                limit: Some(1),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(paged.total, 2);
    assert_eq!(paged.items[0].id, all.items[3].id);
    let bobs = hito
        .catalog
        .liked_recordings(&bob, Search::default())
        .await
        .unwrap();
    assert_eq!(bobs.total, 1);
    assert_eq!(bobs.items[0].id, all.items[0].id);
}
