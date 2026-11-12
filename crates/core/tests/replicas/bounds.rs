use super::fixture::{RemoteServer, local};
use hito_core::{Action, Browse, Operation, SyncClientId};
use uuid::Uuid;

#[test]
fn synchronization_client_identity_is_validated_independently_of_the_host_identity() {
    for invalid in [
        String::new(),
        "x".repeat(129),
        "contains space".into(),
        "url/segment".into(),
    ] {
        assert!(SyncClientId::new(invalid).is_err());
    }
    assert_eq!(
        SyncClientId::new("replica-1_safe").unwrap().as_str(),
        "replica-1_safe"
    );
}

#[tokio::test]
async fn bounded_pushes_and_reads_preserve_every_queued_change() {
    let (_directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    for index in 0..105 {
        let name = format!("{index:04}{}", "\u{1}".repeat(252));
        services
            .replicas
            .enqueue(
                &actor,
                &session,
                Operation {
                    id: Uuid::new_v4(),
                    action: Action::PlaylistCreate {
                        playlist_id: Uuid::new_v4(),
                        name,
                    },
                },
            )
            .await
            .unwrap();
    }
    let first = services
        .replicas
        .synchronize(&actor, &session)
        .await
        .unwrap();
    assert_eq!(first.pushed, 100);
    assert_eq!(first.pending, 5);
    assert!(first.has_more);
    let second = services
        .replicas
        .synchronize(&actor, &session)
        .await
        .unwrap();
    assert_eq!(second.pushed, 5);
    assert_eq!(second.pending, 0);
    assert!(!second.has_more);
    assert_eq!(remote.event_count(), 105);
    let page = services
        .replicas
        .playlists(
            &actor,
            &session,
            Browse {
                offset: Some(60),
                limit: Some(60),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 105);
    assert_eq!(page.items.len(), 45);
    assert_eq!(page.offset, 60);
    assert!(
        services
            .replicas
            .playlists(
                &actor,
                &session,
                Browse {
                    limit: Some(201),
                    ..Default::default()
                }
            )
            .await
            .is_err()
    );
    services.replicas.close().await.unwrap();
}
