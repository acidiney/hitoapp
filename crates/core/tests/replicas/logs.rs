use super::fixture::{RemoteServer, TOKEN, local};
use hito_core::test_support::metadata_tag_logs::CapturedLogs;
use hito_core::{Action, Browse, Operation};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test]
async fn replica_workflows_log_safe_outcomes_and_timing_without_private_inputs() {
    let logs = CapturedLogs::default();
    let (directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    async {
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
        services
            .replicas
            .enqueue(
                &actor,
                &session,
                Operation {
                    id: Uuid::new_v4(),
                    action: Action::PlaylistCreate {
                        playlist_id: Uuid::new_v4(),
                        name: "Private unpublished playlist".into(),
                    },
                },
            )
            .await
            .unwrap();
        services
            .replicas
            .synchronize(&actor, &session)
            .await
            .unwrap();
        services
            .replicas
            .playlists(&actor, &session, Browse::default())
            .await
            .unwrap();
        services
            .replicas
            .disconnect(&actor, connection.id)
            .await
            .unwrap();
        services.replicas.close().await.unwrap();
    }
    .with_subscriber(logs.subscriber())
    .await;
    let text = logs.text();
    for outcome in [
        "connected",
        "activated",
        "queued",
        "synchronized",
        "disconnected",
        "closed",
        "elapsed_ms",
        "request_id",
        "connection_id",
    ] {
        assert!(
            text.contains(outcome),
            "missing safe workflow field {outcome}"
        );
    }
    for private in [
        "Private unpublished playlist",
        "remote-listener",
        "owned-remote-password",
        TOKEN,
        remote.endpoint.as_str(),
        directory.path().to_str().unwrap(),
    ] {
        assert!(
            !text.contains(private),
            "private replica input leaked into logs"
        );
    }
}
