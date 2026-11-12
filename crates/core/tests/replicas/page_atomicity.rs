use super::fixture::{RemoteServer, local};
use hito_core::{Action, Browse, Operation};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn malformed_or_unsupported_pages_commit_neither_partial_playlists_nor_a_cursor() {
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
    let create = json!({"sequence":1,"device_id":"another-client","operation":{
        "id":Uuid::new_v4(),"action":{"kind":"playlist_create","playlist_id":Uuid::new_v4(),"name":"Must remain uncommitted"}
    }});
    let unsupported = json!({"sequence":2,"device_id":"another-client","operation":{
        "id":Uuid::new_v4(),"action":{"kind":"like_set","recording_id":Uuid::new_v4(),"liked":true}
    }});
    for reply in [
        json!({"operations":[create,unsupported],"cursor":2,"has_more":false}),
        json!({"operations":[create],"cursor":2,"has_more":false}),
        json!({"operations":[create,create],"cursor":1,"has_more":false}),
        json!({"operations":[],"cursor":1,"has_more":false}),
        json!({"operations":[],"cursor":0,"has_more":true}),
    ] {
        remote.reply_next_pull(reply);
        assert!(
            services
                .replicas
                .synchronize(&actor, &session)
                .await
                .is_err()
        );
        assert!(
            services
                .replicas
                .playlists(&actor, &session, Browse::default())
                .await
                .unwrap()
                .items
                .is_empty()
        );
        let unchanged = services
            .replicas
            .synchronize(&actor, &session)
            .await
            .unwrap();
        assert_eq!(unchanged.cursor, 0);
        assert_eq!(unchanged.received, 0);
    }
    let rejected = services
        .replicas
        .enqueue(
            &actor,
            &session,
            Operation {
                id: Uuid::new_v4(),
                action: Action::LikeSet {
                    recording_id: Uuid::new_v4(),
                    liked: true,
                },
            },
        )
        .await;
    assert!(rejected.is_err());
    assert_eq!(
        services
            .replicas
            .synchronize(&actor, &session)
            .await
            .unwrap()
            .pending,
        0
    );
    services.replicas.close().await.unwrap();
}
