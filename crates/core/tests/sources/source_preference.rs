use hito_core::test_support::community_ranking_library as library;
use hito_core::test_support::community_rankings as remote;
use hito_core::test_support::content_service as provider;

use hito_core::{
    Actor, ChooseSource, Credentials, Error, Services, Source, SourcePreferenceEntry,
    UpdateCommunityRankingPreferences, UpdateSourcePreferences,
};
use serde_json::json;
use std::future::pending;
use uuid::Uuid;

struct World {
    owned: library::Library,
    youtube: String,
    jamendo: String,
    _servers: (provider::ProviderServer, provider::ProviderServer),
    ranking: remote::RankingServer,
}

impl World {
    async fn start() -> Self {
        let primary = provider::ProviderServer::start().await;
        let candidate = provider::ProviderServer::start().await;
        let ranking = remote::RankingServer::start().await;
        let owned = library::Library::prepare(&primary, &candidate, &ranking.url).await;
        let settings = owned.hito.plugins.settings(&owned.actor).await.unwrap();
        let find = |manifest: &str| {
            settings
                .installations
                .iter()
                .find(|installation| installation.manifest.id == manifest)
                .unwrap()
                .id
                .clone()
        };
        Self {
            youtube: find("org.hito.youtube"),
            jamendo: find("org.hito.jamendo"),
            owned,
            _servers: (primary, candidate),
            ranking,
        }
    }

    fn provider(&self, installation: &str) -> SourcePreferenceEntry {
        SourcePreferenceEntry::Provider {
            installation_id: installation.to_owned(),
        }
    }

    async fn order(&self) -> Vec<&'static str> {
        label_order(
            &self
                .owned
                .hito
                .catalog
                .playback_sources_until(&self.owned.actor, &self.owned.recording.id, pending())
                .await
                .unwrap(),
        )
    }

    async fn save(&self, fallback: bool, entries: Vec<SourcePreferenceEntry>) {
        let hito = &self.owned.hito;
        let current = hito
            .catalog
            .source_preferences(&self.owned.actor)
            .await
            .unwrap();
        hito.catalog
            .update_source_preferences(
                &self.owned.actor,
                UpdateSourcePreferences {
                    revision: current.revision,
                    fallback,
                    entries,
                },
            )
            .await
            .unwrap();
    }

    async fn finish(self) {
        self.ranking.close().await;
        self._servers.0.shutdown().await;
        self._servers.1.shutdown().await;
    }
}

fn label_order(sources: &[Source]) -> Vec<&'static str> {
    sources
        .iter()
        .map(|source| match source.label.as_str() {
            "private-youtube-label" => "youtube",
            "private-jamendo-label" => "jamendo",
            other => panic!("unexpected source label {other}"),
        })
        .collect()
}

async fn second_account(hito: &Services, owner: &Actor) -> Actor {
    hito.identity
        .create_user(
            owner,
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
    hito.identity.authenticate(&session.token).await.unwrap()
}

#[tokio::test]
async fn a_fresh_account_lists_the_defaults_then_its_own_connected_providers() {
    let world = World::start().await;
    let hito = &world.owned.hito;
    let fresh = hito
        .catalog
        .source_preferences(&world.owned.actor)
        .await
        .unwrap();
    assert!(fresh.revision == 0 && fresh.fallback);
    assert_eq!(
        fresh.entries[..2],
        [
            SourcePreferenceEntry::LocalFiles,
            SourcePreferenceEntry::SavedCopies
        ]
    );
    let mut providers: Vec<_> = fresh.entries[2..].to_vec();
    providers.sort_by_key(|entry| format!("{entry:?}"));
    let mut expected = vec![
        world.provider(&world.youtube),
        world.provider(&world.jamendo),
    ];
    expected.sort_by_key(|entry| format!("{entry:?}"));
    assert_eq!(providers, expected);
    let guest = second_account(hito, &world.owned.actor).await;
    let theirs = hito.catalog.source_preferences(&guest).await.unwrap();
    assert_eq!(
        theirs.entries,
        [
            SourcePreferenceEntry::LocalFiles,
            SourcePreferenceEntry::SavedCopies
        ]
    );
    let foreign = hito
        .catalog
        .update_source_preferences(
            &guest,
            UpdateSourcePreferences {
                revision: 0,
                fallback: false,
                entries: vec![world.provider(&world.youtube)],
            },
        )
        .await;
    assert!(matches!(foreign, Err(Error::Invalid(_))));
    world
        .save(
            false,
            vec![
                world.provider(&world.jamendo),
                world.provider(&world.youtube),
            ],
        )
        .await;
    let saved = hito
        .catalog
        .source_preferences(&world.owned.actor)
        .await
        .unwrap();
    assert!(saved.revision == 1 && !saved.fallback);
    assert_eq!(
        saved.entries,
        [
            SourcePreferenceEntry::LocalFiles,
            SourcePreferenceEntry::SavedCopies,
            world.provider(&world.jamendo),
            world.provider(&world.youtube),
        ]
    );
    assert_eq!(
        hito.catalog.source_preferences(&guest).await.unwrap(),
        theirs
    );
    world.finish().await;
}

#[tokio::test]
async fn saves_reject_stale_revisions_duplicates_and_unknown_providers() {
    let world = World::start().await;
    let hito = &world.owned.hito;
    let actor = &world.owned.actor;
    world.save(true, vec![world.provider(&world.youtube)]).await;
    let stale = hito
        .catalog
        .update_source_preferences(
            actor,
            UpdateSourcePreferences {
                revision: 0,
                fallback: true,
                entries: vec![],
            },
        )
        .await;
    assert!(matches!(stale, Err(Error::Conflict(_))));
    for entries in [
        vec![
            SourcePreferenceEntry::LocalFiles,
            SourcePreferenceEntry::LocalFiles,
        ],
        vec![world.provider(&Uuid::new_v4().to_string())],
    ] {
        let rejected = hito
            .catalog
            .update_source_preferences(
                actor,
                UpdateSourcePreferences {
                    revision: 1,
                    fallback: true,
                    entries,
                },
            )
            .await;
        assert!(matches!(rejected, Err(Error::Invalid(_))));
    }
    let kept = hito.catalog.source_preferences(actor).await.unwrap();
    assert!(kept.revision == 1 && kept.entries.contains(&world.provider(&world.youtube)));
    world.finish().await;
}

#[tokio::test]
async fn the_saved_provider_order_decides_between_ready_sources() {
    let world = World::start().await;
    let default = world.order().await;
    assert_eq!(default.len(), 2);
    world
        .save(
            true,
            vec![
                world.provider(&world.jamendo),
                world.provider(&world.youtube),
            ],
        )
        .await;
    assert_eq!(world.order().await, ["jamendo", "youtube"]);
    world
        .save(
            true,
            vec![
                world.provider(&world.youtube),
                world.provider(&world.jamendo),
            ],
        )
        .await;
    assert_eq!(world.order().await, ["youtube", "jamendo"]);
    world.finish().await;
}

#[tokio::test]
async fn a_manual_choice_beats_the_saved_order() {
    let world = World::start().await;
    let hito = &world.owned.hito;
    let actor = &world.owned.actor;
    world
        .save(
            true,
            vec![
                world.provider(&world.youtube),
                world.provider(&world.jamendo),
            ],
        )
        .await;
    let sources = hito
        .catalog
        .sources(actor, &world.owned.recording.id)
        .await
        .unwrap();
    let jamendo = sources
        .iter()
        .find(|source| source.label == "private-jamendo-label")
        .unwrap();
    hito.catalog
        .choose_source(
            actor,
            &world.owned.recording.id,
            ChooseSource {
                request_id: Uuid::new_v4(),
                source_id: Uuid::parse_str(&jamendo.id).unwrap(),
            },
        )
        .await
        .unwrap();
    let listed = hito
        .catalog
        .playback_sources_until(actor, &world.owned.recording.id, pending())
        .await
        .unwrap();
    let ready: Vec<_> = listed.iter().filter(|source| source.ready).collect();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, jamendo.id);
    world.finish().await;
}

#[tokio::test]
async fn community_ranking_never_overrides_the_saved_provider_order() {
    let world = World::start().await;
    let hito = &world.owned.hito;
    let actor = &world.owned.actor;
    let enabled = hito.community.ranking_preferences(actor).await.unwrap();
    hito.community
        .update_ranking_preferences(
            actor,
            UpdateCommunityRankingPreferences {
                revision: enabled.revision,
                use_source_rankings: true,
            },
        )
        .await
        .unwrap();
    let references: Vec<_> = world
        .owned
        .baseline
        .iter()
        .map(library::public_reference)
        .collect();
    let youtube = references
        .iter()
        .find(|reference| reference["provider"] == "youtube")
        .unwrap();
    let jamendo = references
        .iter()
        .find(|reference| reference["provider"] == "jamendo")
        .unwrap();
    world
        .ranking
        .reply(json!({"ranked": [
            {"source": youtube, "successful_reporters": 9},
            {"source": jamendo, "successful_reporters": 1},
        ]}))
        .await;
    assert_eq!(world.order().await, ["youtube", "jamendo"]);
    world
        .save(
            true,
            vec![
                world.provider(&world.jamendo),
                world.provider(&world.youtube),
            ],
        )
        .await;
    assert_eq!(world.order().await, ["jamendo", "youtube"]);
    world.finish().await;
}

#[tokio::test]
async fn candidates_follow_the_saved_provider_order() {
    let world = World::start().await;
    let hito = &world.owned.hito;
    let actor = &world.owned.actor;
    let recording = &world.owned.recording.id;
    let by_label = |label: &str| {
        world
            .owned
            .baseline
            .iter()
            .find(|source| source.label == label)
            .unwrap()
            .id
            .clone()
    };
    for (entries, first, second) in [
        (
            vec![
                world.provider(&world.jamendo),
                world.provider(&world.youtube),
            ],
            by_label("private-jamendo-label"),
            by_label("private-youtube-label"),
        ),
        (
            vec![
                world.provider(&world.youtube),
                world.provider(&world.jamendo),
            ],
            by_label("private-youtube-label"),
            by_label("private-jamendo-label"),
        ),
    ] {
        world.save(true, entries).await;
        let mut candidates = hito
            .playback
            .playback_candidates(actor, recording)
            .await
            .unwrap();
        assert_eq!(candidates.next().await.unwrap().unwrap().source_id(), first);
        assert_eq!(
            candidates.next().await.unwrap().unwrap().source_id(),
            second
        );
        assert!(candidates.next().await.unwrap().is_none());
    }
    world.finish().await;
}

#[tokio::test]
async fn turning_fallback_off_offers_only_the_preferred_candidate_and_says_why() {
    let world = World::start().await;
    let hito = &world.owned.hito;
    world
        .save(
            false,
            vec![
                world.provider(&world.jamendo),
                world.provider(&world.youtube),
            ],
        )
        .await;
    let mut candidates = hito
        .playback
        .playback_candidates(&world.owned.actor, &world.owned.recording.id)
        .await
        .unwrap();
    assert!(candidates.next().await.unwrap().is_some());
    assert!(candidates.ended().is_none());
    assert!(candidates.next().await.unwrap().is_none());
    assert_eq!(
        candidates.ended(),
        Some(hito_core::CandidatesEnd::FallbackOff)
    );
    world.finish().await;
}

async fn choose_by_label(world: &World, actor: &Actor, label: &str) -> String {
    let hito = &world.owned.hito;
    let sources = hito
        .catalog
        .sources(actor, &world.owned.recording.id)
        .await
        .unwrap();
    let chosen = sources.iter().find(|source| source.label == label).unwrap();
    hito.catalog
        .choose_source(
            actor,
            &world.owned.recording.id,
            ChooseSource {
                request_id: Uuid::new_v4(),
                source_id: Uuid::parse_str(&chosen.id).unwrap(),
            },
        )
        .await
        .unwrap();
    chosen.id.clone()
}

async fn offered(world: &World, actor: &Actor) -> Vec<String> {
    let mut candidates = world
        .owned
        .hito
        .playback
        .playback_candidates(actor, &world.owned.recording.id)
        .await
        .unwrap();
    let mut seen = Vec::new();
    while let Some(grant) = candidates.next().await.unwrap() {
        seen.push(grant.source_id().to_owned());
    }
    seen
}

#[tokio::test]
async fn native_candidates_offer_only_the_chosen_provider_source() {
    let world = World::start().await;
    let actor = &world.owned.actor;
    world
        .save(
            true,
            vec![
                world.provider(&world.youtube),
                world.provider(&world.jamendo),
            ],
        )
        .await;
    let jamendo = choose_by_label(&world, actor, "private-jamendo-label").await;
    assert_eq!(offered(&world, actor).await, [jamendo]);
    world.finish().await;
}

#[tokio::test]
async fn a_failed_chosen_source_ends_the_native_candidates_like_the_browser_list() {
    let world = World::start().await;
    let actor = &world.owned.actor;
    let youtube = choose_by_label(&world, actor, "private-youtube-label").await;
    let browser = world
        .owned
        .hito
        .catalog
        .playback_sources_until(actor, &world.owned.recording.id, pending())
        .await
        .unwrap();
    let ready: Vec<_> = browser
        .iter()
        .filter(|source| source.ready)
        .map(|source| source.id.clone())
        .collect();
    assert_eq!(ready, std::slice::from_ref(&youtube));
    let mut candidates = world
        .owned
        .hito
        .playback
        .playback_candidates(actor, &world.owned.recording.id)
        .await
        .unwrap();
    assert_eq!(
        candidates.next().await.unwrap().unwrap().source_id(),
        youtube
    );
    assert!(candidates.next().await.unwrap().is_none());
    assert_eq!(
        candidates.ended(),
        Some(hito_core::CandidatesEnd::Exhausted)
    );
    world.finish().await;
}

#[tokio::test]
async fn another_accounts_choice_does_not_change_native_candidates() {
    let world = World::start().await;
    let actor = &world.owned.actor;
    let before = offered(&world, actor).await;
    assert_eq!(before.len(), 2);
    let guest = second_account(&world.owned.hito, actor).await;
    let denied = |result: hito_core::Result<_>| matches!(result, Err(Error::NotFound));
    assert!(denied(
        world
            .owned
            .hito
            .playback
            .playback_candidates(&guest, &world.owned.recording.id)
            .await
            .map(|_| ())
    ));
    choose_by_label(&world, actor, "private-jamendo-label").await;
    assert_eq!(offered(&world, actor).await.len(), 1);
    world.finish().await;
}
