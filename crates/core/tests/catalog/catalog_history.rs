use hito_core::test_support::support;

use hito_core::{Action, Search};
use support::{catalog, synchronize};
use uuid::Uuid;

#[tokio::test]
async fn history_contains_current_like_state_without_a_separate_likes_list() {
    let (_dir, hito, actor, library) = catalog().await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    let id = Uuid::parse_str(&recordings.items[0].id).unwrap();
    synchronize(
        &hito,
        &actor,
        vec![
            Action::LikeSet {
                recording_id: id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: id,
                position_ms: 700,
                speed: 1.25,
            },
        ],
    )
    .await;
    let activity = serde_json::to_value(hito.collection.activity(&actor).await.unwrap()).unwrap();
    assert!(activity.get("likes").is_none());
    assert_eq!(activity["history"][0]["id"], id.to_string());
    assert_eq!(activity["history"][0]["liked"], true);
    assert_eq!(activity["history"][0]["position_ms"], 700);
    assert_eq!(activity["history"][0]["speed"], 1.25);

    synchronize(
        &hito,
        &actor,
        vec![Action::LikeSet {
            recording_id: id,
            liked: false,
        }],
    )
    .await;
    let activity = serde_json::to_value(hito.collection.activity(&actor).await.unwrap()).unwrap();
    assert_eq!(activity["history"][0]["liked"], false);
    hito.identity
        .set_library_access(&actor, &library, &actor.user().id, false)
        .await
        .unwrap();
    let activity = serde_json::to_value(hito.collection.activity(&actor).await.unwrap()).unwrap();
    assert_eq!(activity, serde_json::json!({"history":[]}));
}
