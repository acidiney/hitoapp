use crate::resolution_musical_fixture as fixture;
use hito_core::test_support::hang_guard::HANG_CEILING;

use hito_core::{Credentials, ResolveSource, SourcePreferenceEntry, UpdateSourcePreferences};
use serde_json::json;
use std::future::pending;
use uuid::Uuid;

const EXTRA_PROVIDERS: usize = 5;

struct World {
    owned: fixture::SavedRecording,
    servers: Vec<fixture::ProviderServer>,
    installations: Vec<String>,
    _primary: fixture::ProviderServer,
}

impl World {
    async fn start() -> Self {
        let primary = fixture::ProviderServer::start().await;
        let first = fixture::ProviderServer::start().await;
        let owned = fixture::SavedRecording::prepare(&primary, &first).await;
        let mut servers = vec![first];
        let mut installations = vec![owned.audio_installation.id.clone()];
        for _ in 0..EXTRA_PROVIDERS {
            let server = fixture::ProviderServer::start().await;
            let installation = connect(&owned, &server).await;
            installations.push(installation);
            servers.push(server);
        }
        for server in &servers {
            server
                .reply("v1/search", json!({"items": [], "next_cursor": null}))
                .await;
        }
        Self {
            owned,
            servers,
            installations,
            _primary: primary,
        }
    }

    fn provider(&self, index: usize) -> SourcePreferenceEntry {
        SourcePreferenceEntry::Provider {
            installation_id: self.installations[index].clone(),
        }
    }

    async fn save(&self, entries: Vec<SourcePreferenceEntry>) {
        save(&self.owned, &self.owned.actor, entries).await;
    }

    async fn asked(&self) -> Vec<usize> {
        tokio::time::timeout(
            HANG_CEILING,
            self.owned.hito.catalog.resolve_source_until(
                &self.owned.actor,
                &self.owned.recording,
                ResolveSource {
                    request_id: Uuid::new_v4(),
                },
                pending(),
            ),
        )
        .await
        .expect("the resolution must settle")
        .unwrap_or_else(|_| panic!("the resolution must return a typed outcome"));
        let mut asked = Vec::new();
        for (index, server) in self.servers.iter().enumerate() {
            if !server.requests("v1/search").await.is_empty() {
                asked.push(index);
            }
        }
        asked
    }

    async fn finish(self) {
        for server in self.servers {
            server.shutdown().await;
        }
    }
}

async fn connect(owned: &fixture::SavedRecording, server: &fixture::ProviderServer) -> String {
    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.youtube-fixture", "name": "Attachment fixture",
                "protocol_version": 1, "capabilities": ["search", "sources", "metadata"]
            }),
        )
        .await;
    let service = owned
        .hito
        .plugins
        .offer_service(
            &owned.actor,
            hito_core::OfferPluginService {
                name: "Attachment fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    owned
        .hito
        .plugins
        .install(
            &owned.actor,
            hito_core::InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
        .id
}

async fn save(
    owned: &fixture::SavedRecording,
    actor: &hito_core::Actor,
    entries: Vec<SourcePreferenceEntry>,
) {
    let current = owned.hito.catalog.source_preferences(actor).await.unwrap();
    owned
        .hito
        .catalog
        .update_source_preferences(
            actor,
            UpdateSourcePreferences {
                revision: current.revision,
                fallback: false,
                entries,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_saved_preference_decides_which_four_of_six_providers_are_asked() {
    let world = World::start().await;
    world
        .save(vec![
            world.provider(5),
            world.provider(3),
            world.provider(1),
            world.provider(4),
            world.provider(0),
        ])
        .await;
    assert_eq!(world.asked().await, vec![1, 3, 4, 5]);
    world.finish().await;
}

#[tokio::test]
async fn without_a_preference_the_first_four_connected_providers_are_asked_every_time() {
    let mut runs = Vec::new();
    for _ in 0..3 {
        let world = World::start().await;
        runs.push(world.asked().await);
        world.finish().await;
    }
    assert_eq!(runs, vec![vec![0, 1, 2, 3]; 3]);
}

#[tokio::test]
async fn another_accounts_preference_does_not_change_the_asked_providers() {
    let world = World::start().await;
    let hito = &world.owned.hito;
    hito.identity
        .create_user(
            &world.owned.actor,
            Credentials {
                username: "guest".into(),
                password: "the guest long password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "guest".into(),
            password: "the guest long password".into(),
        })
        .await
        .unwrap();
    let guest = hito.identity.authenticate(&session.token).await.unwrap();
    world
        .save(vec![
            world.provider(5),
            world.provider(3),
            world.provider(1),
            world.provider(4),
        ])
        .await;
    save(&world.owned, &guest, Vec::new()).await;
    assert_eq!(world.asked().await, vec![1, 3, 4, 5]);
    world.finish().await;
}
