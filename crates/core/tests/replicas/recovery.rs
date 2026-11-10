use super::{
    durable::reopen,
    fixture::{RemoteServer, local},
};
use hito_core::{Action, Browse, Operation};
use uuid::Uuid;

#[tokio::test]
async fn an_uncertain_push_retries_exactly_after_restart_and_an_ack_waits_for_its_echo() {
    for acknowledged in [false, true] {
        let (directory, services, actor) = local().await;
        let remote = RemoteServer::start().await;
        let connection = services
            .replicas
            .connect(&actor, remote.request())
            .await
            .unwrap();
        let lease = services
            .replicas
            .activate(&actor, connection.id)
            .await
            .unwrap();
        services
            .replicas
            .enqueue(
                &actor,
                &lease,
                Operation {
                    id: Uuid::new_v4(),
                    action: Action::PlaylistCreate {
                        playlist_id: Uuid::new_v4(),
                        name: "Retained optimistic list".into(),
                    },
                },
            )
            .await
            .unwrap();
        if acknowledged {
            remote.fail_next_pull();
        } else {
            remote.lose_next_ack();
        }
        assert!(services.replicas.synchronize(&actor, &lease).await.is_err());
        assert_eq!(remote.event_count(), 1);
        assert_eq!(
            services
                .replicas
                .playlists(&actor, &lease, Browse::default())
                .await
                .unwrap()
                .items[0]
                .name,
            "Retained optimistic list"
        );
        services.replicas.close().await.unwrap();
        drop(services);
        let (services, actor) = reopen(directory.path()).await;
        let lease = services
            .replicas
            .activate(&actor, connection.id)
            .await
            .unwrap();
        assert_eq!(
            services
                .replicas
                .playlists(&actor, &lease, Browse::default())
                .await
                .unwrap()
                .total,
            1
        );
        let outcome = services.replicas.synchronize(&actor, &lease).await.unwrap();
        assert_eq!(outcome.pushed, usize::from(!acknowledged));
        assert_eq!(outcome.received, 1);
        assert_eq!(outcome.pending, 0);
        assert_eq!(remote.event_count(), 1);
        services.replicas.close().await.unwrap();
    }
}

#[tokio::test]
async fn two_replicas_converge_in_server_acceptance_order_and_deletion_wins() {
    let (_one, first, a) = local().await;
    let (_two, second, b) = local().await;
    let remote = RemoteServer::start().await;
    let ca = first.replicas.connect(&a, remote.request()).await.unwrap();
    let cb = second.replicas.connect(&b, remote.request()).await.unwrap();
    assert_ne!(ca.client_id, cb.client_id);
    let sa = first.replicas.activate(&a, ca.id).await.unwrap();
    let sb = second.replicas.activate(&b, cb.id).await.unwrap();
    let playlist = Uuid::new_v4();
    first
        .replicas
        .enqueue(
            &a,
            &sa,
            Operation {
                id: Uuid::new_v4(),
                action: Action::PlaylistCreate {
                    playlist_id: playlist,
                    name: "Shared list".into(),
                },
            },
        )
        .await
        .unwrap();
    first.replicas.synchronize(&a, &sa).await.unwrap();
    second.replicas.synchronize(&b, &sb).await.unwrap();
    for (services, actor, session, name) in [
        (&first, &a, &sa, "First rename"),
        (&second, &b, &sb, "Second rename"),
    ] {
        services
            .replicas
            .enqueue(
                actor,
                session,
                Operation {
                    id: Uuid::new_v4(),
                    action: Action::PlaylistRename {
                        playlist_id: playlist,
                        name: name.into(),
                    },
                },
            )
            .await
            .unwrap();
    }
    first.replicas.synchronize(&a, &sa).await.unwrap();
    second.replicas.synchronize(&b, &sb).await.unwrap();
    first.replicas.synchronize(&a, &sa).await.unwrap();
    for (services, actor, session) in [(&first, &a, &sa), (&second, &b, &sb)] {
        assert_eq!(
            services
                .replicas
                .playlists(actor, session, Browse::default())
                .await
                .unwrap()
                .items[0]
                .name,
            "Second rename"
        );
    }
    first
        .replicas
        .enqueue(
            &a,
            &sa,
            Operation {
                id: Uuid::new_v4(),
                action: Action::PlaylistRename {
                    playlist_id: playlist,
                    name: "Late offline rename".into(),
                },
            },
        )
        .await
        .unwrap();
    second
        .replicas
        .enqueue(
            &b,
            &sb,
            Operation {
                id: Uuid::new_v4(),
                action: Action::PlaylistDelete {
                    playlist_id: playlist,
                },
            },
        )
        .await
        .unwrap();
    second.replicas.synchronize(&b, &sb).await.unwrap();
    first.replicas.synchronize(&a, &sa).await.unwrap();
    second.replicas.synchronize(&b, &sb).await.unwrap();
    assert_eq!(remote.event_count(), 5);
    for (services, actor, session) in [(&first, &a, &sa), (&second, &b, &sb)] {
        assert!(
            services
                .replicas
                .playlists(actor, session, Browse::default())
                .await
                .unwrap()
                .items
                .is_empty()
        );
        services.replicas.close().await.unwrap();
    }
}
