use super::{
    collection_artwork::ArtworkServer, database_owner::DatabaseOwner, hang_guard::hang_guard,
};
use crate::{Actor, CoreConfig, Credentials, HostDeviceId, ImportProviderPlaylist, Services};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub struct ArtworkAdmission {
    pub hito: Services,
    pub actor: Actor,
    pub token: String,
    pub recording: String,
    provider: ArtworkServer,
    database: DatabaseOwner,
}

impl ArtworkAdmission {
    pub async fn saturated() -> Self {
        hang_guard(
            "owned saturated artwork fixture",
            Box::pin(async {
                let directory = Arc::new(tempfile::tempdir().unwrap());
                let music = directory.path().join("music");
                std::fs::create_dir(&music).unwrap();
                let database = DatabaseOwner::retain(directory.clone());
                let mut config = CoreConfig::new(
                    directory.path().join("hito.db"),
                    music,
                    HostDeviceId::new("artwork-admission").unwrap(),
                );
                database.attach(&mut config);
                let hito = Box::pin(crate::open(config)).await.unwrap();
                let session = hito
                    .identity
                    .setup(Credentials {
                        username: "admin".into(),
                        password: "indexing test password".into(),
                    })
                    .await
                    .unwrap();
                let actor = hito.identity.authenticate(&session.token).await.unwrap();
                let provider = ArtworkServer::start().await;
                let installed = provider.connect(&hito, &actor).await;
                let mut recording = None;
                for _ in 0..16 {
                    let playlist = hito
                        .collection
                        .import_provider_playlist(
                            &actor,
                            &installed.id,
                            ImportProviderPlaylist {
                                request_id: Uuid::new_v4(),
                                installation_revision: installed.revision,
                                playlist_reference: "https://provider.invalid/owned-artwork".into(),
                            },
                        )
                        .await
                        .unwrap()
                        .playlist_id
                        .to_string();
                    let detail = hito.collection.playlist(&actor, &playlist).await.unwrap();
                    let current = detail.entries[0].recording_id.clone().unwrap();
                    if let Some(recording) = &recording {
                        assert_eq!(&current, recording);
                    } else {
                        recording = Some(current);
                    }
                    let queued = hito
                        .catalog
                        .enqueue_playlist_artwork(&actor, &playlist)
                        .await
                        .unwrap();
                    assert!(matches!(queued.status, crate::ArtworkFetchStatus::Queued));
                }
                assert!(provider.requests().await.is_empty());
                Self {
                    hito,
                    actor,
                    token: session.token,
                    recording: recording.unwrap(),
                    provider,
                    database,
                }
            }),
        )
        .await
    }

    pub async fn close(self) {
        let Self {
            hito,
            provider,
            database,
            ..
        } = self;
        provider.shutdown().await;
        let closed = hito.catalog_replication.close().await;
        drop(hito);
        let retirement = database.retire(Duration::from_secs(3)).await;
        assert!(closed.is_ok());
        assert!(retirement.resources_retired);
        assert!(!retirement.deadline_exceeded);
    }
}
