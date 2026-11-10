use super::{archive, catalog_fixture, owner};
use hito_core::{
    Action, Actor, BeginCatalogSnapshot, Browse, CoreConfig, Credentials, Error, HostDeviceId,
    Operation, ReplicaSession, Services,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use uuid::Uuid;

const PASSWORD: &str = "owned populated backup password";

#[derive(Deserialize, Serialize)]
pub struct Proof {
    pub replica: owner::Proof,
    pub snapshot: Uuid,
    recording: String,
    standalone: Value,
}

pub async fn run(stage: &str) {
    let root = PathBuf::from(std::env::var_os("HITO_CATALOG_BACKUP_ROOT").unwrap());
    let data = PathBuf::from(std::env::var_os("HITO_CATALOG_BACKUP_DATA").unwrap());
    let music = root.join("original-media");
    let manifest = root.join("catalog-expectations.json");
    if stage == "seed" {
        archive::private_file(&data.join("hito.db"), &[]);
    }
    let services = hito_core::open(CoreConfig::new(
        data.join("hito.db"),
        music,
        HostDeviceId::new("owned-catalog-backup-host").unwrap(),
    ))
    .await
    .unwrap();
    let credentials = Credentials {
        username: "owner".into(),
        password: PASSWORD.into(),
    };
    let auth = if stage == "seed" {
        services.identity.setup(credentials).await
    } else {
        services.identity.login(credentials).await
    }
    .unwrap();
    let actor = services.identity.authenticate(&auth.token).await.unwrap();
    if stage == "seed" {
        let endpoint = std::env::var("HITO_CATALOG_BACKUP_ENDPOINT").unwrap();
        let snapshot = std::env::var("HITO_CATALOG_BACKUP_SNAPSHOT")
            .unwrap()
            .parse()
            .unwrap();
        let proof = seed(&services, &actor, endpoint, snapshot).await;
        archive::private_file(&manifest, &serde_json::to_vec(&proof).unwrap());
    } else {
        assert_eq!(stage, "restored");
        let proof: Proof = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        let session = restored_offline(&services, &actor, &proof).await;
        archive::private_file(&root.join("offline-verified"), &[]);
        catalog_fixture::wait_for_signal(&root.join("peer-resumed")).await;
        synchronize(&services, &actor, &session).await;
        assert_catalog(&services, &actor, &session, proof.snapshot).await;
        assert_standalone(&services, &actor, &proof).await;
    }
    services.replicas.close().await.unwrap();
    drop(services);
}

async fn seed(services: &Services, actor: &Actor, endpoint: String, snapshot: Uuid) -> Proof {
    let replica = owner::seed(services, actor, endpoint).await;
    let recording = super::catalog_standalone::seed(services, actor, replica.standalone).await;
    let standalone =
        super::catalog_standalone::snapshot(services, actor, &replica, &recording).await;
    let session = services
        .replicas
        .activate(actor, replica.connection)
        .await
        .unwrap();
    let installed = services
        .replicas
        .install_catalog(
            actor,
            &session,
            BeginCatalogSnapshot {
                library_id: catalog_fixture::LIBRARY.into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(installed, catalog_fixture::catalog(snapshot).0);
    assert_catalog(services, actor, &session, snapshot).await;
    assert!(
        standalone
            == super::catalog_standalone::snapshot(services, actor, &replica, &recording).await,
        "catalog installation changed standalone state"
    );
    Proof {
        replica,
        snapshot,
        recording,
        standalone,
    }
}

async fn restored_offline(services: &Services, actor: &Actor, proof: &Proof) -> ReplicaSession {
    assert_eq!(actor.user().id, proof.replica.owner);
    let connections = services.replicas.connections(actor).await.unwrap();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].id, proof.replica.connection);
    assert_eq!(connections[0].client_id.as_str(), proof.replica.client);
    assert_eq!(connections[0].account.id, super::remote::REMOTE_ACCOUNT);
    assert!(!connections[0].account.admin);
    let session = services
        .replicas
        .activate(actor, proof.replica.connection)
        .await
        .unwrap();
    assert_catalog(services, actor, &session, proof.snapshot).await;
    assert_standalone(services, actor, proof).await;
    let playlists = services
        .replicas
        .playlists(actor, &session, Browse::default())
        .await
        .unwrap();
    assert_eq!(playlists.total, 1);
    assert_eq!(
        playlists.items[0].id,
        proof.replica.remote_playlist.to_string()
    );
    assert!(playlists.items[0].name == owner::OFFLINE_NAME);
    let changed = services
        .replicas
        .enqueue(
            actor,
            &session,
            Operation {
                id: proof.replica.pending,
                action: Action::PlaylistRename {
                    playlist_id: proof.replica.remote_playlist,
                    name: "Changed pending payload".into(),
                },
            },
        )
        .await;
    assert!(matches!(changed, Err(Error::Conflict(_))));
    assert!(
        services
            .replicas
            .install_catalog(
                actor,
                &session,
                BeginCatalogSnapshot {
                    library_id: catalog_fixture::LIBRARY.into(),
                }
            )
            .await
            .is_err(),
        "the restored peer must be unavailable"
    );
    assert_catalog(services, actor, &session, proof.snapshot).await;
    assert_isolated(services, actor, &session, proof.snapshot).await;
    session
}

async fn assert_catalog(services: &Services, actor: &Actor, session: &ReplicaSession, id: Uuid) {
    let (metadata, pages) = catalog_fixture::catalog(id);
    assert_eq!(
        services
            .replicas
            .catalog_snapshot(actor, session, catalog_fixture::LIBRARY)
            .await
            .unwrap()
            .unwrap(),
        metadata
    );
    for expected in pages {
        let page = services
            .replicas
            .catalog_page(actor, session, id, expected.ordinal)
            .await
            .unwrap();
        assert!(
            serde_json::to_value(page).unwrap() == serde_json::to_value(expected).unwrap(),
            "cached catalog facts changed"
        );
    }
    assert!(matches!(
        services.replicas.catalog_page(actor, session, id, 2).await,
        Err(Error::NotFound)
    ));
    assert!(
        services
            .replicas
            .catalog_snapshot(actor, session, "uninstalled-library")
            .await
            .unwrap()
            .is_none()
    );
}

async fn assert_standalone(services: &Services, actor: &Actor, proof: &Proof) {
    assert!(
        super::catalog_standalone::snapshot(services, actor, &proof.replica, &proof.recording)
            .await
            == proof.standalone,
        "standalone metadata, provenance or personal collections changed"
    );
}

async fn assert_isolated(
    services: &Services,
    actor: &Actor,
    session: &ReplicaSession,
    snapshot: Uuid,
) {
    let credentials = || Credentials {
        username: "other".into(),
        password: PASSWORD.into(),
    };
    services
        .identity
        .create_user(actor, credentials())
        .await
        .unwrap();
    let auth = services.identity.login(credentials()).await.unwrap();
    let other = services.identity.authenticate(&auth.token).await.unwrap();
    assert!(
        services
            .replicas
            .connections(&other)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        services
            .replicas
            .catalog_snapshot(&other, session, catalog_fixture::LIBRARY)
            .await
            .is_err()
    );
    assert!(
        services
            .replicas
            .catalog_page(&other, session, snapshot, 0)
            .await
            .is_err()
    );
    assert!(
        services
            .collection
            .playlists(&other)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        services
            .catalog
            .recordings(&other, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
}

async fn synchronize(services: &Services, actor: &Actor, session: &ReplicaSession) {
    let result = services.replicas.synchronize(actor, session).await.unwrap();
    assert_eq!(
        result.pushed, 1,
        "accepted history was resent or pending work was lost"
    );
    assert_eq!(
        result.received, 1,
        "snapshot watermark changed the accepted cursor"
    );
    assert_eq!(result.cursor, 2);
    assert_eq!(result.pending, 0);
    assert!(!result.has_more);
    let next = services.replicas.synchronize(actor, session).await.unwrap();
    assert_eq!(next.pushed, 0);
    assert_eq!(next.received, 0);
    assert_eq!(next.cursor, 2);
}
