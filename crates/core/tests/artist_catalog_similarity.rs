#[allow(dead_code)]
#[path = "fixtures/artist_match_library.rs"]
mod library;

use hito_core::test_support::artist_catalog_similarity_service::{Reply, SimilarityServer};
use hito_core::{
    ArtistRadio, Error, InstallPlugin, MetadataProvider, OfferPluginService, ReadSimilarArtists,
    SimilarArtistsAvailability, UpdateMetadataSettings,
};
use library::{CREDIT, Library, M1, M2};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

const M3: &str = "33333333-3333-4333-8333-333333333333";
const M4: &str = "44444444-4444-4444-8444-444444444444";

fn calls(server: &SimilarityServer) -> Vec<String> {
    server
        .requests()
        .into_iter()
        .map(|request| request.path)
        .filter(|path| !path.ends_with("/v1/manifest"))
        .collect()
}

struct Seed {
    m1_group: String,
    recording: String,
    association: String,
    m2_group: String,
}

async fn seed(library: &Library) -> Seed {
    library.run().await;
    let groups = library
        .hito
        .artist_catalog
        .artist_identity_groups(&library.actor, Default::default())
        .await
        .unwrap()
        .items;
    let m1 = groups.iter().find(|g| g.provider_artist_id == M1).unwrap();
    let m2 = groups.iter().find(|g| g.provider_artist_id == M2).unwrap();
    Seed {
        m1_group: m1.id.clone(),
        recording: m1.name.recording_id.clone(),
        association: m1.name.association_id.clone(),
        m2_group: m2.id.clone(),
    }
}

async fn connect(library: &Library, server: &SimilarityServer) -> MetadataProvider {
    let service = library
        .hito
        .plugins
        .offer_service(
            &library.actor,
            OfferPluginService {
                name: "Owned artist similarity".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = library
        .hito
        .plugins
        .install(
            &library.actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    MetadataProvider::HttpPlugin {
        installation_id: installation.id,
    }
}

async fn choose(library: &Library, enabled: bool, providers: Vec<MetadataProvider>) {
    let current = library
        .hito
        .artist_catalog
        .metadata_settings(&library.actor)
        .await
        .unwrap();
    library
        .hito
        .artist_catalog
        .set_metadata_settings(
            &library.actor,
            UpdateMetadataSettings {
                revision: current.revision,
                enabled,
                providers,
            },
        )
        .await
        .unwrap();
}

async fn read(
    library: &Library,
    seed: &Seed,
    limit: u16,
) -> hito_core::Result<SimilarArtistsAvailability> {
    library
        .hito
        .artist_catalog
        .recording_similar_artists_until(
            &library.actor,
            &seed.recording,
            &seed.association,
            ReadSimilarArtists { limit },
            std::future::pending(),
        )
        .await
}

fn similar(rows: &[(&str, &str, Option<f64>)]) -> Reply {
    Reply::Similar(
        rows.iter()
            .map(|(id, name, score)| (id.to_string(), name.to_string(), *score))
            .collect(),
    )
}

fn available(result: hito_core::Result<SimilarArtistsAvailability>) -> Value {
    match result.unwrap() {
        found @ SimilarArtistsAvailability::Available { .. } => {
            serde_json::to_value(found).unwrap()
        }
        other => panic!(
            "expected available, got {}",
            serde_json::to_string(&other).unwrap()
        ),
    }
}

fn status(result: hito_core::Result<SimilarArtistsAvailability>) -> String {
    serde_json::to_value(result.unwrap()).unwrap()["status"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn a_chosen_capable_plugin_lists_similar_artists_and_links_only_exact_chosen_identities() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let mut server = SimilarityServer::start().await;
    let plugin = connect(&library, &server).await;
    choose(
        &library,
        true,
        vec![MetadataProvider::BuiltinMusicBrainz, plugin.clone()],
    )
    .await;
    server.reply(similar(&[
        (M2, "Nome totalmente diferente", Some(0.9)),
        (M3, "C4 Pedro", None),
    ]));
    let before = library.snapshot().await;

    let found = available(read(&library, &seed, 10).await);

    assert_eq!(found["association_id"], json!(seed.association));
    assert_eq!(found["provider"], serde_json::to_value(&plugin).unwrap());
    let artists = found["artists"].as_array().unwrap();
    assert_eq!(artists.len(), 2);
    assert_eq!(artists[0]["provider_artist_id"], json!(M2));
    assert_eq!(artists[0]["chosen_artist_id"], json!(seed.m2_group));
    assert_eq!(artists[0]["score"], json!(0.9));
    assert_eq!(artists[1]["provider_artist_id"], json!(M3));
    assert_eq!(
        artists[1]["chosen_artist_id"],
        Value::Null,
        "a namesake is never a chosen artist"
    );
    assert_eq!(
        server.similarity_requests(),
        vec![json!({"artist_id": M1, "limit": 10})],
        "only the exact identity and a limit are sent"
    );
    assert_eq!(server.unexpected(), 0);
    assert_eq!(
        library.snapshot().await,
        before,
        "reading similar artists adopts nothing"
    );
    assert!(server.finish().await);
}

#[tokio::test]
async fn without_a_chosen_capable_plugin_the_provider_is_never_contacted() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let mut server = SimilarityServer::start().await;
    let plugin = connect(&library, &server).await;

    assert_eq!(status(read(&library, &seed, 5).await), "unsupported");
    assert!(
        calls(&server).is_empty(),
        "an installed but unchosen plugin is never contacted"
    );

    choose(
        &library,
        true,
        vec![MetadataProvider::BuiltinMusicBrainz, plugin.clone()],
    )
    .await;
    server.declare_similarity(false);
    assert_eq!(status(read(&library, &seed, 5).await), "unsupported");
    assert!(server.similarity_requests().is_empty());

    choose(&library, false, vec![]).await;
    assert_eq!(status(read(&library, &seed, 5).await), "disabled");
    assert!(server.finish().await);
}

#[tokio::test]
async fn invalid_limits_fail_before_any_provider_io() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let mut server = SimilarityServer::start().await;
    let plugin = connect(&library, &server).await;
    choose(
        &library,
        true,
        vec![MetadataProvider::BuiltinMusicBrainz, plugin],
    )
    .await;
    for limit in [0, 26] {
        assert!(matches!(
            read(&library, &seed, limit).await,
            Err(Error::Invalid(_))
        ));
    }
    assert!(calls(&server).is_empty());
    assert!(server.finish().await);
}

#[tokio::test]
async fn provider_priority_decides_and_an_unavailable_or_invalid_provider_falls_through() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let mut broken = SimilarityServer::start().await;
    let mut healthy = SimilarityServer::start().await;
    let first = connect(&library, &broken).await;
    let second = connect(&library, &healthy).await;
    healthy.reply(similar(&[(M3, "Terceira", Some(1.0))]));
    choose(
        &library,
        true,
        vec![
            MetadataProvider::BuiltinMusicBrainz,
            first.clone(),
            second.clone(),
        ],
    )
    .await;

    for bad in [
        Reply::Status(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
        Reply::Raw(
            json!({"artist_id": M1, "items": [{"artist": {"id": "not a canonical id", "name": "x"}, "score": null}]}),
        ),
        Reply::Raw(
            json!({"artist_id": M1, "items": [{"artist": {"id": M1, "name": "self"}, "score": null}]}),
        ),
        Reply::Raw(json!({"artist_id": M2, "items": []})),
    ] {
        broken.reply(bad);
        let found = available(read(&library, &seed, 5).await);
        assert_eq!(found["provider"], serde_json::to_value(&second).unwrap());
    }
    assert_eq!(broken.similarity_requests().len(), 4);

    broken.reply(similar(&[(M4, "Primeira", None)]));
    let found = available(read(&library, &seed, 5).await);
    assert_eq!(found["provider"], serde_json::to_value(&first).unwrap());
    assert_eq!(found["artists"][0]["provider_artist_id"], json!(M4));

    healthy.reply(Reply::Status(axum::http::StatusCode::BAD_GATEWAY));
    broken.reply(Reply::Status(axum::http::StatusCode::BAD_GATEWAY));
    assert_eq!(status(read(&library, &seed, 5).await), "unavailable");
    assert!(broken.finish().await && healthy.finish().await);
}

#[tokio::test]
async fn an_empty_answer_is_available_and_cancellation_stops_the_read() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let mut server = SimilarityServer::start().await;
    let plugin = connect(&library, &server).await;
    choose(
        &library,
        true,
        vec![MetadataProvider::BuiltinMusicBrainz, plugin],
    )
    .await;

    let found = available(read(&library, &seed, 5).await);
    assert_eq!(found["artists"], json!([]));

    server.delay(Duration::from_secs(5));
    let started = std::time::Instant::now();
    let cancelled = library
        .hito
        .artist_catalog
        .recording_similar_artists_until(
            &library.actor,
            &seed.recording,
            &seed.association,
            ReadSimilarArtists { limit: 5 },
            tokio::time::sleep(Duration::from_millis(200)),
        )
        .await
        .unwrap();
    assert!(matches!(cancelled, SimilarArtistsAvailability::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(server.finish().await);
}

#[tokio::test]
async fn other_accounts_and_unreviewed_credits_cannot_read_similar_artists() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let mut server = SimilarityServer::start().await;
    let plugin = connect(&library, &server).await;
    choose(
        &library,
        true,
        vec![MetadataProvider::BuiltinMusicBrainz, plugin],
    )
    .await;

    let peer = library
        .hito
        .artist_catalog
        .recording_similar_artists_until(
            &library.peer,
            &seed.recording,
            &seed.association,
            ReadSimilarArtists { limit: 5 },
            std::future::pending(),
        )
        .await;
    assert!(matches!(peer, Err(Error::NotFound)));
    let wrong = library
        .hito
        .artist_catalog
        .recording_similar_artists_until(
            &library.actor,
            &seed.recording,
            "00000000-0000-4000-8000-000000000000",
            ReadSimilarArtists { limit: 5 },
            std::future::pending(),
        )
        .await;
    assert!(matches!(wrong, Err(Error::NotFound)));
    assert!(calls(&server).is_empty());
    assert!(server.finish().await);
}

#[tokio::test]
async fn a_failing_descriptor_probe_is_unavailable_but_a_missing_one_is_unsupported() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let mut server = SimilarityServer::start().await;
    let plugin = connect(&library, &server).await;
    choose(
        &library,
        true,
        vec![MetadataProvider::BuiltinMusicBrainz, plugin],
    )
    .await;
    server.fail_descriptor(Some(axum::http::StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(status(read(&library, &seed, 5).await), "unavailable");
    server.fail_descriptor(Some(axum::http::StatusCode::NOT_IMPLEMENTED));
    assert_eq!(status(read(&library, &seed, 5).await), "unsupported");
    server.fail_descriptor(None);
    server.declare_similarity(false);
    assert_eq!(status(read(&library, &seed, 5).await), "unsupported");
    assert!(server.finish().await);
}

#[tokio::test]
async fn providers_that_cannot_answer_do_not_use_up_the_answer_attempts() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let mut plain = Vec::new();
    let mut providers = vec![MetadataProvider::BuiltinMusicBrainz];
    for _ in 0..3 {
        let server = SimilarityServer::start().await;
        server.declare_similarity(false);
        providers.push(connect(&library, &server).await);
        plain.push(server);
    }
    let mut capable = SimilarityServer::start().await;
    let fourth = connect(&library, &capable).await;
    providers.push(fourth.clone());
    capable.reply(similar(&[(M3, "Quarta", None)]));
    choose(&library, true, providers).await;
    let found = available(read(&library, &seed, 5).await);
    assert_eq!(found["provider"], serde_json::to_value(&fourth).unwrap());
    for server in &plain {
        assert!(server.similarity_requests().is_empty());
    }
    for mut server in plain {
        assert!(server.finish().await);
    }
    assert!(capable.finish().await);
}

async fn group_tracks(library: &Library, group: &str) -> BTreeSet<String> {
    library
        .hito
        .artist_catalog
        .artist_identity_group(&library.actor, group, Default::default())
        .await
        .unwrap()
        .recordings
        .items
        .into_iter()
        .map(|member| member.recording.id)
        .collect()
}

fn radio_ids(radio: &ArtistRadio) -> Vec<String> {
    radio.tracks.iter().map(|track| track.id.clone()).collect()
}

#[tokio::test]
async fn radio_without_a_similarity_provider_plays_only_the_artists_library_tracks() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let own = group_tracks(&library, &seed.m1_group).await;
    assert!(own.len() >= 2);
    let before = library.snapshot().await;

    let radio = library
        .hito
        .artist_catalog
        .artist_radio(&library.actor, &seed.m1_group)
        .await
        .unwrap();

    let ids = radio_ids(&radio);
    assert_eq!(ids.iter().cloned().collect::<BTreeSet<_>>(), own);
    assert_eq!(ids.len(), own.len(), "no track repeats");
    assert_eq!(radio.similar_artists, 0);
    assert!(radio.artist.starts_with(CREDIT), "{}", radio.artist);
    assert_eq!(library.snapshot().await, before, "radio changes nothing");
}

#[tokio::test]
async fn radio_mixes_in_library_tracks_of_chosen_similar_artists_only() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let mut server = SimilarityServer::start().await;
    let plugin = connect(&library, &server).await;
    choose(
        &library,
        true,
        vec![MetadataProvider::BuiltinMusicBrainz, plugin],
    )
    .await;
    server.reply(similar(&[
        (M2, "Nome totalmente diferente", Some(0.9)),
        (M3, "Not in the library", Some(0.5)),
    ]));
    let own = group_tracks(&library, &seed.m1_group).await;
    let similar_tracks = group_tracks(&library, &seed.m2_group).await;
    assert!(!similar_tracks.is_empty());

    let radio = library
        .hito
        .artist_catalog
        .artist_radio(&library.actor, &seed.m1_group)
        .await
        .unwrap();

    let expected: BTreeSet<String> = own.union(&similar_tracks).cloned().collect();
    let ids = radio_ids(&radio);
    assert_eq!(ids.iter().cloned().collect::<BTreeSet<_>>(), expected);
    assert_eq!(ids.len(), expected.len());
    assert_eq!(
        radio.similar_artists, 1,
        "only chosen artists contribute tracks"
    );
    assert_eq!(
        server.similarity_requests(),
        vec![json!({"artist_id": M1, "limit": 25})]
    );
    assert_eq!(server.unexpected(), 0);
    assert!(server.finish().await);
}

#[tokio::test]
async fn radio_falls_back_to_the_library_when_the_similarity_provider_fails() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    let mut server = SimilarityServer::start().await;
    let plugin = connect(&library, &server).await;
    choose(
        &library,
        true,
        vec![MetadataProvider::BuiltinMusicBrainz, plugin],
    )
    .await;
    server.reply(Reply::Status(axum::http::StatusCode::BAD_GATEWAY));
    let own = group_tracks(&library, &seed.m1_group).await;

    let radio = library
        .hito
        .artist_catalog
        .artist_radio(&library.actor, &seed.m1_group)
        .await
        .unwrap();

    assert_eq!(radio_ids(&radio).into_iter().collect::<BTreeSet<_>>(), own);
    assert_eq!(radio.similar_artists, 0);
    assert!(server.finish().await);
}

#[tokio::test]
async fn radio_needs_a_chosen_artist_of_the_account() {
    let library = Library::prepare().await;
    let seed = seed(&library).await;
    for (actor, group) in [
        (&library.actor, "not-a-group"),
        (&library.peer, seed.m1_group.as_str()),
    ] {
        assert!(matches!(
            library.hito.artist_catalog.artist_radio(actor, group).await,
            Err(Error::NotFound)
        ));
    }
}
