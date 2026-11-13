use hito_core::{
    Actor, CoreConfig, Credentials, HostDeviceId, InstallPlugin, OfferPluginService,
    PluginInstallation, Services,
    test_support::{database_owner::DatabaseOwner, health_service::HealthServer},
};
use hito_plugin_sdk::{HealthReason, HealthReport, HealthState};
use std::{sync::Arc, time::Duration};

pub struct Case {
    pub owner: DatabaseOwner,
    pub hito: Services,
    pub actor: Actor,
    pub peer: Actor,
    pub server: HealthServer,
    pub installed: PluginInstallation,
    pub peer_installation: String,
}

impl Case {
    pub async fn new() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let owner = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("health-deadline-host").unwrap(),
        );
        owner.attach(&mut config);
        let hito = hito_core::open(config).await.unwrap();
        let session = hito.identity.setup(credentials("admin")).await.unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        hito.identity
            .create_user(&actor, credentials("health-peer"))
            .await
            .unwrap();
        let session = hito
            .identity
            .login(credentials("health-peer"))
            .await
            .unwrap();
        let peer = hito.identity.authenticate(&session.token).await.unwrap();
        let server = HealthServer::start(
            "Owned deadline service",
            HealthReport {
                state: HealthState::Degraded,
                reason: Some(HealthReason::Network),
                since: Some(1_700_000_017),
            },
        )
        .await;
        server.require_bearer("owned-health-deadline-credential");
        let service = hito
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Owned deadline service".into(),
                    base_url: server.url.clone(),
                },
            )
            .await
            .unwrap();
        let request = || InstallPlugin {
            service_id: service.id.clone(),
            service_revision: service.revision,
            accept_manifest_digest: None,
            bearer_token: Some("owned-health-deadline-credential".into()),
        };
        let installed = hito.plugins.install(&actor, request()).await.unwrap();
        let peer_installation = hito.plugins.install(&peer, request()).await.unwrap().id;
        Self {
            owner,
            hito,
            actor,
            peer,
            server,
            installed,
            peer_installation,
        }
    }

    pub async fn close(self) -> bool {
        let downloads = self.hito.downloads.finish_downloads().await.is_ok();
        let playback = self.hito.playback.close().await.is_ok();
        let replicas = self.hito.replicas.close().await.is_ok();
        let catalog = self.hito.catalog_replication.close().await.is_ok();
        drop(self.hito);
        let retired = self.owner.retire(Duration::from_secs(5)).await;
        downloads
            && playback
            && replicas
            && catalog
            && retired.resources_retired
            && !retired.deadline_exceeded
            && self.owner.observe().active_resources() == 0
    }
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: "owned health deadline password".into(),
    }
}
