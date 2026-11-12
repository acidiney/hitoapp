use hito_core::{
    Action, Actor, Browse, ConnectReplica, CoreConfig, Credentials, Error, HostDeviceId, Operation,
    Services, SyncBatch,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

pub const BASE_NAME: &str = "Accepted remote playlist";
pub const OFFLINE_NAME: &str = "Durable offline rename";
const STANDALONE_NAME: &str = "Independent standalone playlist";
const PASSWORD: &str = "owned stopped backup fixture password";

#[derive(Deserialize, Serialize)]
pub struct Proof {
    pub owner: String,
    pub connection: Uuid,
    pub client: String,
    pub standalone: Uuid,
    pub standalone_operation: Uuid,
    pub remote_playlist: Uuid,
    pub accepted: Uuid,
    pub pending: Uuid,
}

pub async fn run(stage: &str) {
    let data = PathBuf::from(std::env::var_os("HITO_REPLICA_BACKUP_DATA").unwrap());
    let music = PathBuf::from(std::env::var_os("HITO_REPLICA_BACKUP_MUSIC").unwrap());
    let manifest = PathBuf::from(std::env::var_os("HITO_REPLICA_BACKUP_MANIFEST").unwrap());
    let endpoint = std::env::var("HITO_REPLICA_BACKUP_ENDPOINT").unwrap();
    if stage == "seed" {
        super::archive::private_file(&data.join("hito.db"), &[]);
    }
    let services = hito_core::open(CoreConfig::new(
        data.join("hito.db"),
        music,
        HostDeviceId::new("owned-backup-host").unwrap(),
    ))
    .await
    .unwrap();
    let credentials = || Credentials {
        username: "owner".into(),
        password: PASSWORD.into(),
    };
    let auth = if stage == "seed" {
        services.identity.setup(credentials()).await
    } else {
        services.identity.login(credentials()).await
    }
    .unwrap();
    let actor = services.identity.authenticate(&auth.token).await.unwrap();
    match stage {
        "seed" => {
            let proof = seed(&services, &actor, endpoint).await;
            let bytes = serde_json::to_vec(&proof).unwrap();
            super::archive::private_file(&manifest, &bytes);
        }
        "restored" => {
            let proof: Proof = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
            restored(&services, &actor, &proof).await;
        }
        _ => panic!("unsupported owned backup stage"),
    }
    services.replicas.close().await.unwrap();
    drop(services);
}

pub(super) async fn seed(services: &Services, actor: &Actor, endpoint: String) -> Proof {
    let standalone = Uuid::new_v4();
    let standalone_operation = Uuid::new_v4();
    services
        .collection
        .apply(
            actor,
            SyncBatch {
                device_id: "owned-standalone-client".into(),
                operations: vec![Operation {
                    id: standalone_operation,
                    action: Action::PlaylistCreate {
                        playlist_id: standalone,
                        name: STANDALONE_NAME.into(),
                    },
                }],
            },
        )
        .await
        .unwrap();
    let connection = services
        .replicas
        .connect(
            actor,
            ConnectReplica {
                endpoint,
                credentials: Credentials {
                    username: "remote-listener".into(),
                    password: "owned-remote-password".into(),
                },
            },
        )
        .await
        .unwrap();
    let session = services
        .replicas
        .activate(actor, connection.id)
        .await
        .unwrap();
    let remote_playlist = Uuid::new_v4();
    let accepted = Uuid::new_v4();
    services
        .replicas
        .enqueue(
            actor,
            &session,
            Operation {
                id: accepted,
                action: Action::PlaylistCreate {
                    playlist_id: remote_playlist,
                    name: BASE_NAME.into(),
                },
            },
        )
        .await
        .unwrap();
    let synced = services
        .replicas
        .synchronize(actor, &session)
        .await
        .unwrap();
    assert_eq!(synced.pushed, 1);
    assert_eq!(synced.received, 1);
    assert_eq!(synced.pending, 0);
    assert_eq!(synced.cursor, 1);
    let pending = Uuid::new_v4();
    services
        .replicas
        .enqueue(
            actor,
            &session,
            Operation {
                id: pending,
                action: Action::PlaylistRename {
                    playlist_id: remote_playlist,
                    name: OFFLINE_NAME.into(),
                },
            },
        )
        .await
        .unwrap();
    let page = services
        .replicas
        .playlists(actor, &session, Browse::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert!(page.items[0].name == OFFLINE_NAME);
    Proof {
        owner: actor.user().id.clone(),
        connection: connection.id,
        client: connection.client_id.as_str().into(),
        standalone,
        standalone_operation,
        remote_playlist,
        accepted,
        pending,
    }
}

async fn restored(services: &Services, actor: &Actor, proof: &Proof) {
    assert_eq!(actor.user().id, proof.owner);
    let standalone = services.collection.playlists(actor).await.unwrap();
    assert_eq!(standalone.len(), 1);
    assert_eq!(standalone[0].id, proof.standalone.to_string());
    assert!(standalone[0].name == STANDALONE_NAME);
    let events = services.collection.events(actor, 0).await.unwrap();
    assert_eq!(events.operations.len(), 1);
    match &events.operations[0] {
        hito_core::SyncEvent::Operation { operation, .. } => {
            assert_eq!(operation.id, proof.standalone_operation)
        }
        _ => panic!("restoration changed standalone history"),
    }
    let connections = services.replicas.connections(actor).await.unwrap();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].id, proof.connection);
    assert_eq!(connections[0].client_id.as_str(), proof.client);
    assert_eq!(connections[0].account.id, super::remote::REMOTE_ACCOUNT);
    assert!(!connections[0].account.admin);
    let session = services
        .replicas
        .activate(actor, proof.connection)
        .await
        .unwrap();
    let page = services
        .replicas
        .playlists(actor, &session, Browse::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, proof.remote_playlist.to_string());
    assert!(page.items[0].name == OFFLINE_NAME);
    let changed = services
        .replicas
        .enqueue(
            actor,
            &session,
            Operation {
                id: proof.pending,
                action: Action::PlaylistRename {
                    playlist_id: proof.remote_playlist,
                    name: "A changed payload must be rejected".into(),
                },
            },
        )
        .await;
    assert!(
        matches!(changed, Err(Error::Conflict(_))),
        "the pending operation identity was not preserved"
    );
    let synced = services
        .replicas
        .synchronize(actor, &session)
        .await
        .unwrap();
    assert_eq!(
        synced.pushed, 1,
        "accepted history was resent or the pending operation was lost"
    );
    assert_eq!(synced.received, 1, "the accepted cursor was not preserved");
    assert_eq!(synced.cursor, 2);
    assert_eq!(synced.pending, 0);
    assert!(!synced.has_more);
    let page = services
        .replicas
        .playlists(actor, &session, Browse::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert!(page.items[0].name == OFFLINE_NAME);
    let next = services
        .replicas
        .synchronize(actor, &session)
        .await
        .unwrap();
    assert_eq!(next.pushed, 0);
    assert_eq!(next.received, 0);
    assert_eq!(next.cursor, 2);
}
