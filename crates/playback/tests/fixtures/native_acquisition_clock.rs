use futures_util::FutureExt;
use hito_core::{
    AttachReviewedProviderSource, CoreConfig, Credentials, HostDeviceId, ProviderCandidate,
    SourcePreferenceEntry, UpdateSourcePreferences,
    test_support::{
        content_service::{HeldAudioTail, ProviderServer},
        database_owner::DatabaseOwner,
        hang_guard::hang_guard,
        playlist_service, support,
    },
};
use serde_json::json;
use std::{f64::consts::TAU, panic::AssertUnwindSafe, sync::Arc, time::Duration};
use uuid::Uuid;

pub struct Fixture {
    pub services: hito_core::Services,
    pub actor: hito_core::Actor,
    pub first: ProviderServer,
    pub second: ProviderServer,
    pub recording: Uuid,
    pub sentinel: Uuid,
    pub first_installation_id: String,
    pub second_installation_id: String,
    pub first_source_id: String,
    pub second_source_id: String,
    directory: Arc<tempfile::TempDir>,
    music: std::path::PathBuf,
    database: DatabaseOwner,
    original_file: Vec<u8>,
    sentinel_file: Vec<u8>,
    original_modified: std::time::SystemTime,
    sentinel_modified: std::time::SystemTime,
    history_before: Vec<(String, i64, bool)>,
    sentinel_position_before: Option<u64>,
    history_enabled_before: bool,
    preferences_before: hito_core::SourcePreferences,
}

pub struct Cleanup {
    pub first_provider_closed: bool,
    pub second_provider_closed: bool,
    pub database_retired: bool,
    pub database_deadline_exceeded: bool,
}

impl Fixture {
    pub async fn start() -> Self {
        hang_guard("native acquisition owned fixture", async {
            let directory = Arc::new(tempfile::tempdir().unwrap());
            let music = directory.path().join("music");
            std::fs::create_dir(&music).unwrap();
            let database = DatabaseOwner::retain(directory.clone());
            let mut config = CoreConfig::new(
                directory.path().join("hito.db"),
                music.clone(),
                HostDeviceId::new("native-acquisition-clock").unwrap(),
            );
            database.attach(&mut config);
            let services = hito_core::open(config).await.unwrap();
            let session = services
                .identity
                .setup(Credentials {
                    username: "admin".into(),
                    password: "owned acquisition clock password".into(),
                })
                .await
                .unwrap();
            let actor = services
                .identity
                .authenticate(&session.token)
                .await
                .unwrap();
            let library_id = services
                .catalog
                .libraries(&actor)
                .await
                .unwrap()
                .into_iter()
                .find(|library| !library.personal)
                .unwrap()
                .id;
            let original_file = support::write_wav(&music.join("original.wav"), 10);
            let sentinel_file = wave(880.0);
            std::fs::write(music.join("sentinel.wav"), &sentinel_file).unwrap();
            support::scan(&services, &actor, &library_id).await;
            let sentinel = services
                .catalog
                .recordings(&actor, Default::default())
                .await
                .unwrap()
                .items
                .into_iter()
                .find(|recording| {
                    recording.library_id == library_id && recording.title == "sentinel"
                })
                .unwrap();
            let sentinel = Uuid::parse_str(&sentinel.id).unwrap();

            let first = ProviderServer::start().await;
            let first_installation =
                playlist_service::connect(&services, &actor, &first, &["search", "sources"]).await;
            first
                .reply(
                    "v1/recordings/detail",
                    json!({
                        "id": "recording", "title": "Fixture track", "artist": "Fixture artist",
                        "duration_ms": 90000, "revision": "primary-r1"
                    }),
                )
                .await;
            first
                .reply(
                    "v1/sources",
                    json!({"items": [{
                        "id": "audio/first", "label": "First", "ready": true
                    }]}),
                )
                .await;
            let target = services
                .catalog
                .adopt_provider(
                    &actor,
                    &first_installation.id,
                    hito_core::AdoptProviderRecording {
                        installation_revision: first_installation.revision,
                        provider_recording_id: "recording".into(),
                    },
                )
                .await
                .unwrap();
            let recording = Uuid::parse_str(&target.id).unwrap();
            let first_source_id = services
                .catalog
                .sources(&actor, &target.id)
                .await
                .unwrap()
                .into_iter()
                .find(|source| source.label == "First")
                .unwrap()
                .id;

            let second = ProviderServer::start().await;
            let second_installation =
                playlist_service::connect(&services, &actor, &second, &["search", "sources"]).await;
            second
                .reply(
                    "v1/recordings/detail",
                    json!({
                        "id": "alternate", "title": "Fixture track", "artist": "Fixture artist",
                        "duration_ms": 90000, "revision": "alternate-r1"
                    }),
                )
                .await;
            second
                .reply(
                    "v1/sources",
                    json!({"items": [{
                        "id": "audio/second", "label": "Second", "ready": true
                    }]}),
                )
                .await;
            let candidate = ProviderCandidate {
                installation_id: second_installation.id.clone(),
                installation_revision: second_installation.revision,
                provider_recording_id: "alternate".into(),
            };
            let review = services
                .catalog
                .review_provider_source(&actor, &target.id, candidate.clone())
                .await
                .unwrap();
            services
                .catalog
                .attach_provider_source(
                    &actor,
                    &target.id,
                    AttachReviewedProviderSource {
                        candidate,
                        review_stamp: review.review_stamp,
                    },
                )
                .await
                .unwrap();
            let attached_sources = services.catalog.sources(&actor, &target.id).await.unwrap();
            let second_source_id = attached_sources
                .iter()
                .find(|source| source.label == "Second")
                .unwrap()
                .id
                .clone();
            assert!(
                attached_sources
                    .iter()
                    .any(|source| source.id == first_source_id)
            );

            let preferences = services.catalog.source_preferences(&actor).await.unwrap();
            services
                .catalog
                .update_source_preferences(
                    &actor,
                    UpdateSourcePreferences {
                        revision: preferences.revision,
                        fallback: true,
                        entries: vec![
                            SourcePreferenceEntry::Provider {
                                installation_id: first_installation.id.clone(),
                            },
                            SourcePreferenceEntry::Provider {
                                installation_id: second_installation.id.clone(),
                            },
                        ],
                    },
                )
                .await
                .unwrap();
            let preferences_before = services.catalog.source_preferences(&actor).await.unwrap();
            let activity = services.collection.activity(&actor).await.unwrap();
            let history_before = activity
                .history
                .into_iter()
                .map(|item| (item.id, item.position_ms, item.liked))
                .collect();
            let sentinel_position_before = services
                .collection
                .saved_position(&actor, sentinel)
                .await
                .unwrap()
                .map(|saved| saved.position_ms);
            let history_enabled_before = services
                .collection
                .history_preference(&actor)
                .await
                .unwrap()
                .enabled;
            let original_modified = std::fs::metadata(music.join("original.wav"))
                .unwrap()
                .modified()
                .unwrap();
            let sentinel_modified = std::fs::metadata(music.join("sentinel.wav"))
                .unwrap()
                .modified()
                .unwrap();
            Self {
                services,
                actor,
                first,
                second,
                recording,
                sentinel,
                first_installation_id: first_installation.id,
                second_installation_id: second_installation.id,
                first_source_id,
                second_source_id,
                directory,
                music,
                database,
                original_file,
                sentinel_file,
                original_modified,
                sentinel_modified,
                history_before,
                sentinel_position_before,
                history_enabled_before,
                preferences_before,
            }
        })
        .await
    }

    pub fn hold_second_tail(&self, prefix: usize) -> Arc<HeldAudioTail> {
        self.second.hold_audio_tail(prefix)
    }

    pub async fn preserved(&self, sentinel_was_heard: bool) -> bool {
        let activity = self
            .services
            .collection
            .activity(&self.actor)
            .await
            .unwrap();
        let history_after = activity
            .history
            .into_iter()
            .map(|item| (item.id, item.position_ms, item.liked))
            .collect::<Vec<_>>();
        let sentinel_id = self.sentinel.to_string();
        let before_other = self
            .history_before
            .iter()
            .filter(|(id, _, _)| id != &sentinel_id)
            .cloned()
            .collect::<Vec<_>>();
        let after_other = history_after
            .iter()
            .filter(|(id, _, _)| id != &sentinel_id)
            .cloned()
            .collect::<Vec<_>>();
        let preferences_after = self
            .services
            .catalog
            .source_preferences(&self.actor)
            .await
            .unwrap();
        let sentinel_position_after = self
            .services
            .collection
            .saved_position(&self.actor, self.sentinel)
            .await
            .unwrap()
            .map(|saved| saved.position_ms);
        let history_enabled_after = self
            .services
            .collection
            .history_preference(&self.actor)
            .await
            .unwrap()
            .enabled;
        let sentinel_state_unchanged = if sentinel_was_heard {
            sentinel_position_after.is_some()
        } else {
            sentinel_position_after == self.sentinel_position_before
                && history_after == self.history_before
        };
        self.directory.path().exists()
            && std::fs::read(self.music.join("original.wav")).unwrap() == self.original_file
            && std::fs::read(self.music.join("sentinel.wav")).unwrap() == self.sentinel_file
            && std::fs::metadata(self.music.join("original.wav"))
                .unwrap()
                .modified()
                .unwrap()
                == self.original_modified
            && std::fs::metadata(self.music.join("sentinel.wav"))
                .unwrap()
                .modified()
                .unwrap()
                == self.sentinel_modified
            && before_other == after_other
            && sentinel_state_unchanged
            && history_enabled_after == self.history_enabled_before
            && preferences_after == self.preferences_before
    }

    pub async fn close(self) -> Cleanup {
        let Fixture {
            first,
            second,
            services,
            database,
            directory,
            ..
        } = self;
        let first_provider_closed = AssertUnwindSafe(first.shutdown())
            .catch_unwind()
            .await
            .is_ok();
        let second_provider_closed = AssertUnwindSafe(second.shutdown())
            .catch_unwind()
            .await
            .is_ok();
        drop(services);
        let retirement = AssertUnwindSafe(database.retire(Duration::from_secs(5)))
            .catch_unwind()
            .await;
        drop(database);
        drop(directory);
        match retirement {
            Ok(result) => Cleanup {
                first_provider_closed,
                second_provider_closed,
                database_retired: result.resources_retired,
                database_deadline_exceeded: result.deadline_exceeded,
            },
            Err(_) => Cleanup {
                first_provider_closed,
                second_provider_closed,
                database_retired: false,
                database_deadline_exceeded: true,
            },
        }
    }
}

pub fn wave(frequency: f64) -> Vec<u8> {
    let mut file = hito_audio::test_support::wave(48_000, 2, 48_000 * 30, |frame, _| {
        (f64::sin(TAU * frequency * f64::from(frame) / 48_000.0) * 12_000.0) as i16
    });
    use std::io::Read;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    bytes
}
