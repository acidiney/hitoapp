use super::artist_track_audio::{
    BONUS_OCCURRENCE, audio_provider, connect_audio, find, library::Album, offer,
};
use hito_core::AlbumTrackAudio;
use hito_core::test_support::album_edition_service::AlbumProvider;
use serde_json::json;

#[tokio::test]
async fn a_catalog_track_uses_the_first_ranked_ready_candidate_as_a_persisted_best_guess() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(
        &audio,
        "audio-provider/other",
        "Different catalog title",
        "Owned album performer",
        1000,
    )
    .await;
    let total = album.total().await;

    let result = find(&album, BONUS_OCCURRENCE).await.unwrap();
    assert_eq!(
        result.outcome(),
        "saved",
        "Ranked ready audio must be saved after strict matching is exhausted"
    );
    let AlbumTrackAudio::Saved { recording } = result else {
        unreachable!()
    };
    let sources = album
        .hito
        .catalog
        .sources(&album.actor, &recording.id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert!(sources[0].ready);
    assert_eq!(
        serde_json::to_value(&sources[0]).unwrap()["match_kind"],
        json!("best_guess")
    );
    assert_eq!(album.total().await, total + 1);
    assert_eq!(audio.requests("v1/search").await.len(), 1);
    // One hydration and the existing adoption freshness check.
    for path in ["v1/recordings/detail", "v1/sources"] {
        assert_eq!(audio.requests(path).await.len(), 2, "{path}");
    }
    assert_eq!(audio.requests("v1/recordings/metadata").await.len(), 1);
    assert!(audio.requests("v1/audio").await.is_empty());
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

pub(super) async fn ranked(audio: &audio_provider::ProviderServer, results: &[(&str, &str, bool)]) {
    let mut summaries = Vec::new();
    for (id, title, ready) in results {
        let source = format!("{id}/source");
        let [detail, metadata, mut sources] = super::artist_track_audio::candidate(
            id,
            title,
            hito_core::test_support::album_edition_service::CREDIT,
            1000,
            &source,
        );
        sources["items"][0]["ready"] = json!(ready);
        summaries.push(detail.clone());
        for (path, value) in [
            ("v1/recordings/detail", detail),
            ("v1/recordings/metadata", metadata),
            ("v1/sources", sources),
        ] {
            audio.recording_reply(path, id, value).await;
        }
    }
    audio
        .reply(
            "v1/search",
            json!({"items": summaries, "next_cursor": null}),
        )
        .await;
}

#[tokio::test]
async fn a_manually_saved_first_candidate_is_preserved_and_the_next_ranked_candidate_is_saved() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    let installation = connect_audio(&album, &audio).await;
    ranked(
        &audio,
        &[
            ("audio/first", "Other title", true),
            ("audio/second", "Another title", true),
        ],
    )
    .await;
    let first = album
        .hito
        .catalog
        .adopt_provider(
            &album.actor,
            &installation.id,
            hito_core::AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: "audio/first".into(),
            },
        )
        .await
        .unwrap();
    let before = album
        .hito
        .catalog
        .recording_metadata(&album.actor, &first.id, Default::default())
        .await
        .unwrap();
    let total = album.total().await;
    ranked(&audio, &[("audio/first", "Other title", true)]).await;
    assert!(matches!(
        find(&album, BONUS_OCCURRENCE).await.unwrap(),
        AlbumTrackAudio::NoMatch
    ));
    assert_eq!(album.total().await, total);
    ranked(
        &audio,
        &[
            ("audio/first", "Other title", true),
            ("audio/second", "Another title", true),
        ],
    )
    .await;

    let result = find(&album, BONUS_OCCURRENCE).await.unwrap();
    assert_eq!(
        result.outcome(),
        "saved",
        "A known occupied candidate must not hide the next usable result"
    );
    let AlbumTrackAudio::Saved { recording } = result else {
        unreachable!()
    };
    assert_eq!(recording.title, "Another title");
    assert_ne!(recording.id, first.id);
    let sources = album
        .hito
        .catalog
        .sources(&album.actor, &recording.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&sources[0]).unwrap()["match_kind"],
        "best_guess"
    );
    let original = album
        .hito
        .catalog
        .sources(&album.actor, &first.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&original[0]).unwrap()["match_kind"],
        "verified"
    );
    let after = album
        .hito
        .catalog
        .recording_metadata(&album.actor, &first.id, Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    assert_eq!(album.total().await, total + 1);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn a_strict_catalog_match_wins_over_a_higher_ranked_mismatch() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    ranked(
        &audio,
        &[
            ("audio/first", "Other title", true),
            ("audio/second", super::artist_track_audio::BONUS_TITLE, true),
        ],
    )
    .await;
    let AlbumTrackAudio::Saved { recording } = find(&album, BONUS_OCCURRENCE).await.unwrap() else {
        panic!("A strict match must win")
    };
    assert_eq!(recording.title, super::artist_track_audio::BONUS_TITLE);
    let sources = album
        .hito
        .catalog
        .sources(&album.actor, &recording.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&sources[0]).unwrap()["match_kind"],
        "verified"
    );
    assert_eq!(
        audio
            .requests("v1/recordings/detail")
            .await
            .iter()
            .filter(|(_, body)| body["recording_id"] == "audio/first")
            .count(),
        1
    );
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn ranked_catalog_candidates_need_ready_sources_and_valid_metadata_witnesses() {
    for failure in ["unready", "wrong_witness", "no_witness"] {
        let mut catalog = AlbumProvider::start().await;
        let audio = audio_provider::ProviderServer::start().await;
        let album = Album::prepare(&catalog).await;
        connect_audio(&album, &audio).await;
        ranked(
            &audio,
            &[
                ("audio/first", "Other title", failure != "unready"),
                ("audio/second", "Another title", true),
            ],
        )
        .await;
        if failure != "unready" {
            let [_, mut metadata, _] = super::artist_track_audio::candidate(
                "audio/first",
                "Other title",
                hito_core::test_support::album_edition_service::CREDIT,
                1000,
                "audio/first/source",
            );
            metadata["source_ids"] = if failure == "no_witness" {
                json!(null)
            } else {
                json!(["unrelated/source"])
            };
            audio
                .recording_reply("v1/recordings/metadata", "audio/first", metadata)
                .await;
        }
        let AlbumTrackAudio::Saved { recording } = find(&album, BONUS_OCCURRENCE).await.unwrap()
        else {
            panic!("A technically invalid first result must not hide the next one")
        };
        assert_eq!(recording.title, "Another title", "{failure}");
        let sources = album
            .hito
            .catalog
            .sources(&album.actor, &recording.id)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&sources[0]).unwrap()["match_kind"],
            "best_guess"
        );
        assert_eq!(audio.requests("v1/search").await.len(), 1);
        assert_eq!(
            audio.requests("v1/recordings/metadata").await.len(),
            2,
            "Hydrated metadata must be reused"
        );
        audio.shutdown().await;
        assert!(catalog.finish().await);
    }
}

#[tokio::test]
async fn ranked_catalog_search_keeps_the_configured_provider_order() {
    let mut catalog = AlbumProvider::start().await;
    let first = audio_provider::ProviderServer::start().await;
    let preferred = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    let a = connect_audio(&album, &first).await;
    let b = audio_provider::connect(
        &album.hito,
        &album.actor,
        &preferred,
        "org.hito.preferred-audio-fixture",
        &["search", "sources", "metadata"],
    )
    .await;
    ranked(&first, &[("audio/first", "First provider title", true)]).await;
    ranked(
        &preferred,
        &[("audio/preferred", "Preferred provider title", true)],
    )
    .await;
    let preferences = album
        .hito
        .catalog
        .source_preferences(&album.actor)
        .await
        .unwrap();
    album
        .hito
        .catalog
        .update_source_preferences(
            &album.actor,
            hito_core::UpdateSourcePreferences {
                revision: preferences.revision,
                fallback: false,
                entries: vec![
                    hito_core::SourcePreferenceEntry::Provider {
                        installation_id: b.id,
                    },
                    hito_core::SourcePreferenceEntry::Provider {
                        installation_id: a.id,
                    },
                ],
            },
        )
        .await
        .unwrap();
    let AlbumTrackAudio::Saved { recording } = find(&album, BONUS_OCCURRENCE).await.unwrap() else {
        panic!("The preferred provider must supply the best guess")
    };
    assert_eq!(recording.title, "Preferred provider title");
    for server in [&first, &preferred] {
        assert_eq!(server.requests("v1/search").await.len(), 1);
    }
    let sources = album
        .hito
        .catalog
        .sources(&album.actor, &recording.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&sources[0]).unwrap()["match_kind"],
        "best_guess"
    );
    first.shutdown().await;
    preferred.shutdown().await;
    assert!(catalog.finish().await);
}
