use hito_core::test_support::support;

use hito_core::{Action, Credentials, Error, MetadataHistoryQuery, Operation, Search, SyncBatch};
use support::{catalog, synchronize};
use uuid::Uuid;

#[tokio::test]
async fn personal_metadata_and_its_provenance_require_both_account_and_library_access() {
    let (_dir, hito, actor, library) = catalog().await;
    let track = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording = Uuid::parse_str(&track.id).unwrap();
    let bob = hito
        .identity
        .create_user(
            &actor,
            Credentials {
                username: "bob".into(),
                password: "metadata bob password".into(),
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
            password: "metadata bob password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    synchronize(
        &hito,
        &actor,
        vec![Action::RecordingTitleSet {
            recording_id: recording,
            title: "Private constellation".into(),
        }],
    )
    .await;
    let detail = hito
        .catalog
        .recording_metadata(&actor, &track.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(
        detail.title.personal.as_deref(),
        Some("Private constellation")
    );
    assert_eq!(detail.title.inherited, track.title);
    assert!(detail.title.operation_id.is_some());
    let detail = hito
        .catalog
        .recording_metadata(&bob, &track.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(detail.title.value, track.title);
    assert!(detail.title.personal.is_none() && detail.title.operation_id.is_none());
    assert!(
        !serde_json::to_string(&detail)
            .unwrap()
            .contains("Private constellation")
    );
    assert_eq!(
        hito.catalog
            .recordings(
                &bob,
                Search {
                    q: Some("Private constellation".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        hito.catalog
            .recordings(
                &actor,
                Search {
                    q: Some("Private constellation".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        1
    );
    synchronize(
        &hito,
        &bob,
        vec![Action::RecordingTitleSet {
            recording_id: recording,
            title: "Bob's title".into(),
        }],
    )
    .await;
    hito.identity
        .set_library_access(&actor, &library, &bob.user().id, false)
        .await
        .unwrap();
    assert!(matches!(
        hito.catalog
            .recording_metadata(&bob, &track.id, MetadataHistoryQuery::default())
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        hito.catalog
            .recordings(&bob, Search::default())
            .await
            .unwrap()
            .total,
        0
    );
    hito.identity
        .set_library_access(&actor, &library, &bob.user().id, true)
        .await
        .unwrap();
    assert_eq!(
        hito.catalog
            .recording_metadata(&bob, &track.id, MetadataHistoryQuery::default())
            .await
            .unwrap()
            .title
            .value,
        "Bob's title"
    );
}

#[tokio::test]
async fn invalid_title_batch_rolls_back_metadata_search_and_acceptance() {
    let (_dir, hito, actor, _library) = catalog().await;
    let track = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording = Uuid::parse_str(&track.id).unwrap();
    for title in [" ".to_string(), "Title\nSecond".into(), "é".repeat(1025)] {
        let result = hito
            .collection
            .apply(
                &actor,
                SyncBatch {
                    device_id: "title-client".into(),
                    operations: vec![
                        Operation {
                            id: Uuid::new_v4(),
                            action: Action::RecordingTitleSet {
                                recording_id: recording,
                                title: "Rolled back".into(),
                            },
                        },
                        Operation {
                            id: Uuid::new_v4(),
                            action: Action::RecordingTitleSet {
                                recording_id: recording,
                                title,
                            },
                        },
                    ],
                },
            )
            .await;
        assert!(matches!(result, Err(Error::Invalid(_))));
        assert_eq!(
            hito.catalog
                .recording_metadata(&actor, &track.id, MetadataHistoryQuery::default())
                .await
                .unwrap()
                .title
                .value,
            track.title
        );
        assert_eq!(
            hito.catalog
                .recordings(
                    &actor,
                    Search {
                        q: Some("Rolled back".into()),
                        ..Default::default()
                    }
                )
                .await
                .unwrap()
                .total,
            0
        );
        assert!(
            hito.collection
                .events(&actor, 0)
                .await
                .unwrap()
                .operations
                .is_empty()
        );
    }
}
