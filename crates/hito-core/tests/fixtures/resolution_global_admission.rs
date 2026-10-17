use crate::provider::Provider;
use hito_core::{
    Actor, AdoptProviderRecording, Credentials, Error, InstallPlugin, OfferPluginService,
    PluginInstallation, Services,
};
use serde_json::Value;

pub struct Account {
    pub actor: Actor,
    pub recording: String,
    selected: Value,
}

impl Account {
    pub async fn prepare(
        hito: &Services,
        admin: &Actor,
        provider: &Provider,
        username: &str,
    ) -> Self {
        let credentials = || Credentials {
            username: username.into(),
            password: "owned global admission password".into(),
        };
        hito.identity
            .create_user(admin, credentials())
            .await
            .unwrap_or_else(|_| panic!("the independent account must be created"));
        let session = hito
            .identity
            .login(credentials())
            .await
            .unwrap_or_else(|_| panic!("the independent account must log in"));
        let actor = hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap_or_else(|_| panic!("the independent account must authenticate"));
        assert!(
            !actor.user().admin,
            "each listening account must be independent of the administrator"
        );
        let primary = connect(hito, admin, &actor, provider.metadata_url(), None).await;
        connect(
            hito,
            admin,
            &actor,
            provider.audio_url(),
            Some("owned-resolution-private-token".into()),
        )
        .await;
        let recording = hito
            .catalog
            .adopt_provider(
                &actor,
                &primary.id,
                AdoptProviderRecording {
                    installation_revision: primary.revision,
                    provider_recording_id: "owned/primary".into(),
                },
            )
            .await
            .unwrap_or_else(|_| panic!("each account must adopt its own target"));
        assert!(
            !recording.available,
            "each target must initially have no ready source"
        );
        assert!(provider.events().await.is_empty());
        let selected = hito
            .catalog
            .recording_metadata(&actor, &recording.id, Default::default())
            .await
            .unwrap_or_else(|_| panic!("the primary metadata observation must be readable"))
            .selected_observation;
        Self {
            actor,
            recording: recording.id,
            selected: serde_json::to_value(selected).unwrap(),
        }
    }

    pub async fn assert_metadata(&self, hito: &Services) {
        let selected = hito
            .catalog
            .recording_metadata(&self.actor, &self.recording, Default::default())
            .await
            .unwrap_or_else(|_| panic!("each account must retain its primary metadata"))
            .selected_observation;
        assert!(
            serde_json::to_value(selected).unwrap() == self.selected,
            "admission and automatic source publication must preserve primary metadata"
        );
    }

    pub async fn assert_private_catalog(&self, hito: &Services, accounts: &[Self]) {
        let catalog = hito
            .catalog
            .recordings(&self.actor, Default::default())
            .await
            .unwrap_or_else(|_| panic!("each independent account must read its own catalog"));
        assert_eq!(catalog.total, 1);
        assert!(catalog.items[0].id == self.recording && catalog.items[0].available);
        for other in accounts {
            if other.actor.user().id != self.actor.user().id {
                assert!(
                    matches!(
                        hito.catalog.sources(&self.actor, &other.recording).await,
                        Err(Error::NotFound)
                    ),
                    "global capacity sharing must not grant access to another account's sources"
                );
            }
        }
    }
}

async fn connect(
    hito: &Services,
    admin: &Actor,
    actor: &Actor,
    base_url: String,
    bearer_token: Option<String>,
) -> PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            admin,
            OfferPluginService {
                name: "Owned global admission service".into(),
                base_url,
            },
        )
        .await
        .unwrap_or_else(|_| panic!("the administrator must offer the owned plugin service"));
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token,
            },
        )
        .await
        .unwrap_or_else(|_| panic!("each account must install its approved plugin"))
}
