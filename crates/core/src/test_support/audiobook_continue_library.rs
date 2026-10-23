use super::{content_service as provider, support};
use crate::{
    Action, Actor, AdoptProviderRecording, ChooseSource, Credentials, InstallPlugin,
    OfferPluginService, Services,
};
use serde_json::{Value, json};
use uuid::Uuid;

pub struct Library {
    pub directory: tempfile::TempDir,
    pub services: Services,
    pub admin: Actor,
    pub actor: Actor,
    pub server: provider::ProviderServer,
    pub book: Uuid,
    pub entries: [Uuid; 4],
    pub recordings: [Uuid; 3],
    pub original_path: std::path::PathBuf,
    pub original: Vec<u8>,
}
impl Library {
    pub async fn start() -> Self {
        let (directory, services, admin, shared, music) = support::library("continue-grants").await;
        let original_path = music.join("local.wav");
        let original = support::write_wav(&original_path, 123);
        support::scan(&services, &admin, &shared).await;
        let local = services
            .catalog
            .recordings(&admin, Default::default())
            .await
            .unwrap()
            .items
            .remove(0);
        let credentials = Credentials {
            username: "continue-listener".into(),
            password: "private continue listener password".into(),
        };
        let user = services
            .identity
            .create_user(
                &admin,
                Credentials {
                    username: credentials.username.clone(),
                    password: credentials.password.clone(),
                },
            )
            .await
            .unwrap();
        services
            .identity
            .set_library_access(&admin, &shared, &user.id, true)
            .await
            .unwrap();
        let login = services.identity.login(credentials).await.unwrap();
        let actor = services.identity.authenticate(&login.token).await.unwrap();
        assert!(!actor.user().admin);
        let server = provider::ProviderServer::start().await;
        let offered = services
            .plugins
            .offer_service(
                &admin,
                OfferPluginService {
                    name: "Audiobook preparation fixture".into(),
                    base_url: server.url.clone(),
                },
            )
            .await
            .unwrap();
        let installation = services
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: offered.id,
                    service_revision: 1,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        let mut personal = Vec::new();
        for (id, ready) in [("ready-chapter", true), ("pending-chapter", false)] {
            server
                .recording_reply(
                    "v1/recordings/detail",
                    id,
                    json!({
                        "id": id, "title": id, "artist": "Fixture narrator", "duration_ms": 10_000,
                        "revision": "r1",
                    }),
                )
                .await;
            server
                .recording_reply(
                    "v1/sources",
                    id,
                    json!({"items": [{
                        "id": format!("audio-{id}"), "label": "Original", "ready": ready,
                    }]}),
                )
                .await;
            personal.push(
                services
                    .catalog
                    .adopt_provider(
                        &actor,
                        &installation.id,
                        AdoptProviderRecording {
                            installation_revision: 1,
                            provider_recording_id: id.into(),
                        },
                    )
                    .await
                    .unwrap(),
            );
        }
        assert!(personal[0].available && !personal[1].available);
        let recordings = [
            personal[0].id.parse().unwrap(),
            local.id.parse().unwrap(),
            personal[1].id.parse().unwrap(),
        ];
        let sources = services
            .catalog
            .sources(&actor, &personal[0].id)
            .await
            .unwrap();
        services
            .catalog
            .choose_source(
                &actor,
                &personal[0].id,
                ChooseSource {
                    request_id: Uuid::new_v4(),
                    source_id: sources[0].id.parse().unwrap(),
                },
            )
            .await
            .unwrap();
        let book = Uuid::new_v4();
        let entries = std::array::from_fn(|_| Uuid::new_v4());
        let mut actions = vec![Action::PlaylistCreate {
            playlist_id: book,
            name: "Private mixed-authority book".into(),
        }];
        for (index, recording_id) in [recordings[0], recordings[1], recordings[2], recordings[0]]
            .into_iter()
            .enumerate()
        {
            actions.push(Action::EntryInsert {
                playlist_id: book,
                entry_id: entries[index],
                recording_id,
                position: index as i64,
            });
        }
        actions.extend([
            Action::AudiobookCreate { playlist_id: book },
            Action::RecordingTitleSet {
                recording_id: recordings[0],
                title: "Private corrected chapter title".into(),
            },
            Action::RecordingArtistSet {
                recording_id: recordings[0],
                artist: "Private corrected narrator".into(),
            },
            Action::LikeSet {
                recording_id: recordings[2],
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recordings[0],
                position_ms: 100,
                speed: 1.25,
            },
            Action::AudiobookChapterSet {
                chapter: crate::AudiobookChapterAnchor {
                    audiobook_id: book,
                    entry_id: entries[0],
                    recording_id: recordings[0],
                },
            },
        ]);
        support::synchronize(&services, &actor, actions).await;
        services
            .identity
            .set_library_access(&admin, &shared, actor.user().id.as_str(), false)
            .await
            .unwrap();
        Self {
            directory,
            services,
            admin,
            actor,
            server,
            book,
            entries,
            recordings,
            original_path,
            original,
        }
    }

    pub async fn snapshot(&self) -> Value {
        json!({
            "book": self.services.collection.audiobook(&self.actor, &self.book.to_string()).await.unwrap(),
            "catalog": self.services.catalog.recordings(&self.actor, Default::default()).await.unwrap(),
            "events": self.services.collection.events(&self.actor, 0).await.unwrap(),
            "metadata": self.services.catalog.recording_metadata(&self.actor, &self.recordings[0].to_string(), Default::default()).await.unwrap(),
            "source_choice": self.services.catalog.source_choice(&self.actor, &self.recordings[0].to_string()).await.unwrap(),
            "progress": self.services.collection.saved_playback(&self.actor, &[self.recordings[0], self.recordings[2]]).await.unwrap(),
        })
    }
}
