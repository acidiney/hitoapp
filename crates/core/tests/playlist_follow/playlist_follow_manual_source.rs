use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Actor, AttachReviewedProviderSource, ChooseSource, FollowProviderPlaylist,
    PlaylistFollowInterval, PluginInstallation, PluginRefresh, ProviderCandidate,
    RefreshFollowedPlaylist, RefreshPlugin, ResolveSource, Services, SourceResolutionOutcome,
};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

struct Followed {
    _directory: tempfile::TempDir,
    hito: Services,
    actor: Actor,
    server: fixture::ProviderServer,
    alternate: fixture::ProviderServer,
    installation: PluginInstallation,
    playlist: String,
    recording: String,
    chosen: String,
    revision: i64,
}

fn track(id: &str, title: &str) -> Value {
    json!({"item": {"kind": "recording", "recording": {
        "id": id, "title": title, "artist": "Fixture artist",
        "duration_ms": 90_000, "revision": "r1"
    }}})
}

fn tracks(ids: &[(&str, &str)]) -> Vec<Value> {
    ids.iter().map(|(id, title)| track(id, title)).collect()
}

async fn serve(server: &fixture::ProviderServer, snapshot: &str, items: Vec<Value>) {
    fixture::playlist(server, "playlist/manual", snapshot, items).await;
}

async fn followed(name: &str) -> Followed {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _shared, _music) = support::library(name).await;
    let alternate = fixture::ProviderServer::start().await;
    let installation =
        fixture::connect(&hito, &actor, &server, &["playlists", "search", "sources"]).await;
    let other = fixture::connect(&hito, &actor, &alternate, &["search", "sources"]).await;
    let first = json!({
        "id": "recording/A", "title": "Track A", "artist": "Fixture artist",
        "duration_ms": 90_000, "revision": "r1"
    });
    serve(
        &server,
        "one",
        tracks(&[("recording/A", "Track A"), ("recording/B", "Track B")]),
    )
    .await;
    server
        .reply("v1/search", json!({"items": [first], "next_cursor": null}))
        .await;
    server.reply("v1/recordings/detail", first).await;
    server
        .reply(
            "v1/sources",
            json!({"items": [
                {"id": "audio/first", "label": "First", "ready": true}
            ]}),
        )
        .await;
    alternate
        .reply(
            "v1/recordings/detail",
            json!({
                "id": "alternate/A", "title": "Alternate A", "artist": "Fixture artist",
                "duration_ms": 90_000, "revision": "r1"
            }),
        )
        .await;
    alternate
        .reply(
            "v1/sources",
            json!({"items": [{"id": "audio/second", "label": "Second", "ready": true}]}),
        )
        .await;
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/manual-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let detail = hito.collection.playlist(&actor, &playlist).await.unwrap();
    let recording = detail.entries[0].recording_id.clone().unwrap();
    let resolved = hito
        .catalog
        .resolve_source_until(
            &actor,
            &recording,
            ResolveSource {
                request_id: Uuid::new_v4(),
            },
            std::future::pending(),
        )
        .await
        .unwrap();
    assert!(matches!(resolved.outcome, SourceResolutionOutcome::Ready));
    let sources = hito.catalog.sources(&actor, &recording).await.unwrap();
    assert_eq!(sources.len(), 1);
    let candidate = ProviderCandidate {
        installation_id: other.id.clone(),
        installation_revision: other.revision,
        provider_recording_id: "alternate/A".into(),
    };
    let review = hito
        .catalog
        .review_provider_source(&actor, &recording, candidate.clone())
        .await
        .unwrap();
    hito.catalog
        .attach_provider_source(
            &actor,
            &recording,
            AttachReviewedProviderSource {
                candidate,
                review_stamp: review.review_stamp,
            },
        )
        .await
        .unwrap();
    let sources = hito.catalog.sources(&actor, &recording).await.unwrap();
    assert_eq!(sources.len(), 2);
    let second = sources
        .iter()
        .find(|source| source.label == "Second")
        .unwrap();
    let receipt = hito
        .catalog
        .choose_source(
            &actor,
            &recording,
            ChooseSource {
                request_id: Uuid::new_v4(),
                source_id: Uuid::parse_str(&second.id).unwrap(),
            },
        )
        .await
        .unwrap();
    Followed {
        _directory: directory,
        hito,
        actor,
        server,
        alternate,
        installation: other,
        playlist,
        recording,
        chosen: second.id.clone(),
        revision: receipt.revision,
    }
}

impl Followed {
    async fn refresh(&self) {
        self.hito
            .collection
            .enqueue_follow_refresh(
                &self.actor,
                &self.playlist,
                RefreshFollowedPlaylist {
                    request_id: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();
        assert!(
            self.hito
                .collection
                .run_next_follow_refresh_until(std::future::pending())
                .await
                .unwrap()
        );
    }

    async fn choice(&self) -> Value {
        serde_json::to_value(
            self.hito
                .catalog
                .source_choice(&self.actor, &self.recording)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    async fn ready_ids(&self) -> Vec<String> {
        self.hito
            .catalog
            .sources(&self.actor, &self.recording)
            .await
            .unwrap()
            .into_iter()
            .filter(|source| source.ready)
            .map(|source| source.id)
            .collect()
    }

    async fn entries(&self) -> Vec<Option<String>> {
        self.hito
            .collection
            .playlist(&self.actor, &self.playlist)
            .await
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.recording_id)
            .collect()
    }

    async fn source_requests(&self) -> usize {
        let mut total = 0;
        for server in [&self.server, &self.alternate] {
            total += server.requests("v1/sources").await.len()
                + server.requests("v1/search").await.len();
        }
        total
    }

    fn assert_kept(&self, choice: &Value) {
        assert_eq!(choice["mode"], "manual");
        assert_eq!(choice["revision"], self.revision);
        let selected = choice["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|source| source["selected"] == true)
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0]["source"]["id"], self.chosen.as_str());
    }

    async fn choose_other(&mut self) {
        let sources = self
            .hito
            .catalog
            .source_choice(&self.actor, &self.recording)
            .await
            .unwrap()
            .sources;
        let other = sources
            .iter()
            .find(|entry| entry.source.id != self.chosen)
            .unwrap();
        self.chosen = other.source.id.clone();
        self.revision = self
            .hito
            .catalog
            .choose_source(
                &self.actor,
                &self.recording,
                ChooseSource {
                    request_id: Uuid::new_v4(),
                    source_id: Uuid::parse_str(&self.chosen).unwrap(),
                },
            )
            .await
            .unwrap()
            .revision;
    }

    async fn set_capabilities(&mut self, capabilities: &[&str]) {
        self.alternate
            .reply(
                "v1/manifest",
                json!({
                    "id": "org.hito.playlist-fixture", "name": "Playlist fixture",
                    "protocol_version": 1, "capabilities": capabilities
                }),
            )
            .await;
        let PluginRefresh::ReviewRequired { digest, .. } = self
            .hito
            .plugins
            .refresh(
                &self.actor,
                &self.installation.id,
                RefreshPlugin {
                    revision: self.installation.revision,
                    accept_manifest_digest: None,
                    credential: Default::default(),
                },
            )
            .await
            .unwrap()
        else {
            panic!("A changed manifest must require review");
        };
        let PluginRefresh::Updated { installation } = self
            .hito
            .plugins
            .refresh(
                &self.actor,
                &self.installation.id,
                RefreshPlugin {
                    revision: self.installation.revision,
                    accept_manifest_digest: Some(digest),
                    credential: Default::default(),
                },
            )
            .await
            .unwrap()
        else {
            panic!("The reviewed manifest must be accepted");
        };
        self.installation = installation;
    }

    async fn resolve(&self) -> SourceResolutionOutcome {
        self.hito
            .catalog
            .resolve_source_until(
                &self.actor,
                &self.recording,
                ResolveSource {
                    request_id: Uuid::new_v4(),
                },
                std::future::pending(),
            )
            .await
            .unwrap()
            .outcome
    }
}

#[tokio::test]
async fn a_follow_refresh_that_keeps_the_entry_keeps_the_manual_choice_without_resolving_again() {
    let f = followed("follow-manual-keep").await;
    let before = f.choice().await;
    f.assert_kept(&before);
    let requests = f.source_requests().await;

    serve(
        &f.server,
        "two",
        tracks(&[
            ("recording/A", "Track A"),
            ("recording/B", "Track B"),
            ("recording/C", "Track C"),
        ]),
    )
    .await;
    f.refresh().await;

    assert_eq!(f.entries().await.len(), 3);
    assert_eq!(f.entries().await[0].as_deref(), Some(f.recording.as_str()));
    assert_eq!(f.choice().await, before);
    assert_eq!(f.source_requests().await, requests);
    assert!(matches!(
        f.resolve().await,
        SourceResolutionOutcome::ProtectedChoice
    ));
    assert_eq!(f.source_requests().await, requests);
}

#[tokio::test]
async fn removing_reordering_and_readding_the_provider_track_keeps_its_manual_choice() {
    let f = followed("follow-manual-reorder").await;
    let before = f.choice().await;

    serve(
        &f.server,
        "two",
        tracks(&[("recording/B", "Track B"), ("recording/A", "Track A")]),
    )
    .await;
    f.refresh().await;
    assert_eq!(f.entries().await[1].as_deref(), Some(f.recording.as_str()));
    assert_eq!(f.choice().await, before);

    serve(&f.server, "three", tracks(&[("recording/B", "Track B")])).await;
    f.refresh().await;
    assert_eq!(f.entries().await.len(), 1);

    serve(
        &f.server,
        "four",
        tracks(&[("recording/B", "Track B"), ("recording/A", "Track A")]),
    )
    .await;
    f.refresh().await;
    assert_eq!(f.entries().await[1].as_deref(), Some(f.recording.as_str()));
    let after = f.choice().await;
    f.assert_kept(&after);
    assert_eq!(after, before);
    assert!(matches!(
        f.resolve().await,
        SourceResolutionOutcome::ProtectedChoice
    ));
}

#[tokio::test]
async fn a_choice_made_while_a_follow_refresh_is_in_flight_survives_the_publication() {
    let mut f = followed("follow-manual-during").await;
    f.choose_other().await;
    f.choose_other().await;
    let held = f.revision;
    let before = f.choice().await;
    f.assert_kept(&before);

    serve(
        &f.server,
        "two",
        tracks(&[("recording/B", "Track B"), ("recording/A", "Track A")]),
    )
    .await;
    f.hito
        .collection
        .enqueue_follow_refresh(
            &f.actor,
            &f.playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    let gate = f.server.hold("v1/playlists/items").await;
    let collection = f.hito.collection.clone();
    let worker = collection.run_next_follow_refresh_until(std::future::pending());
    tokio::pin!(worker);
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            biased;
            _ = gate.entered() => {},
            _ = &mut worker => panic!("The refresh must reach its held provider request"),
        }
    })
    .await
    .expect("The provider must observe the running refresh");

    f.choose_other().await;
    assert_eq!(f.revision, held + 1);
    gate.release();
    assert!(worker.await.unwrap());

    let after = f.choice().await;
    f.assert_kept(&after);
    assert_eq!(f.entries().await[1].as_deref(), Some(f.recording.as_str()));
}

#[tokio::test]
async fn a_choice_made_after_a_follow_refresh_publishes_keeps_the_revision_it_received() {
    let mut f = followed("follow-manual-after").await;
    serve(
        &f.server,
        "two",
        tracks(&[("recording/B", "Track B"), ("recording/A", "Track A")]),
    )
    .await;
    f.refresh().await;
    f.choose_other().await;
    let choice = f.choice().await;
    f.assert_kept(&choice);
    f.refresh().await;
    assert_eq!(f.choice().await, choice);
}

#[tokio::test]
async fn a_revoked_capability_shows_the_manual_choice_as_unavailable_and_recovery_restores_it() {
    let mut f = followed("follow-manual-eligibility").await;
    let before = f.choice().await;

    f.set_capabilities(&["search"]).await;
    let revoked = f.choice().await;
    assert_eq!(revoked["mode"], "manual");
    assert_eq!(revoked["revision"], f.revision);
    let entry = revoked["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["source"]["id"] == f.chosen.as_str())
        .expect("The chosen source must stay visible while unavailable");
    assert_eq!(entry["selected"], true);
    assert_eq!(entry["selectable"], false);
    assert!(
        revoked["sources"]
            .as_array()
            .unwrap()
            .iter()
            .all(|source| source["selected"] == (source["source"]["id"] == f.chosen.as_str()))
    );
    assert!(f.ready_ids().await.is_empty());
    assert!(matches!(
        f.resolve().await,
        SourceResolutionOutcome::ProtectedChoice
    ));

    serve(
        &f.server,
        "two",
        tracks(&[("recording/B", "Track B"), ("recording/A", "Track A")]),
    )
    .await;
    f.refresh().await;
    assert_eq!(f.choice().await["sources"], revoked["sources"]);

    f.set_capabilities(&["search", "sources"]).await;
    let recovered = f.choice().await;
    assert_eq!(recovered, before);
    f.assert_kept(&recovered);
    assert_eq!(
        f.ready_ids().await.as_slice(),
        std::slice::from_ref(&f.chosen)
    );
}

#[tokio::test]
async fn removing_the_connection_never_replaces_the_manual_choice_with_an_automatic_source() {
    let f = followed("follow-manual-removed").await;
    let before = f.choice().await;
    f.hito
        .plugins
        .remove(&f.actor, &f.installation.id, f.installation.revision)
        .await
        .unwrap();
    let removed = f.choice().await;
    assert_eq!(removed["mode"], "manual");
    assert_eq!(removed["revision"], f.revision);
    let entry = removed["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["source"]["id"] == f.chosen.as_str())
        .unwrap();
    assert_eq!(entry["selected"], true);
    assert_eq!(entry["selectable"], false);
    assert!(f.ready_ids().await.is_empty());
    assert_eq!(before["mode"], "manual");
    assert!(matches!(
        f.resolve().await,
        SourceResolutionOutcome::ProtectedChoice
    ));
    assert!(f.ready_ids().await.is_empty());
}

#[tokio::test]
async fn automatic_retries_after_a_provider_outage_do_not_replace_the_manual_choice() {
    let f = followed("follow-manual-outage").await;
    let before = f.choice().await;
    f.server.offline().await;
    f.alternate.offline().await;
    assert!(matches!(
        f.resolve().await,
        SourceResolutionOutcome::ProtectedChoice
    ));
    let _ = f.hito.catalog.refresh_sources(&f.actor, &f.recording).await;
    assert_eq!(f.choice().await, before);

    serve(&f.server, "two", tracks(&[("recording/A", "Track A")])).await;
    f.hito
        .collection
        .enqueue_follow_refresh(
            &f.actor,
            &f.playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    let _ = f
        .hito
        .collection
        .run_next_follow_refresh_until(std::future::pending())
        .await;
    assert_eq!(f.choice().await, before);

    f.server.online().await;
    f.alternate.online().await;
    assert!(matches!(
        f.resolve().await,
        SourceResolutionOutcome::ProtectedChoice
    ));
    assert_eq!(f.choice().await, before);
    assert_eq!(
        f.ready_ids().await.as_slice(),
        std::slice::from_ref(&f.chosen)
    );
}
