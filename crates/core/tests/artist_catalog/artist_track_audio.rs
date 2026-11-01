#[path = "../fixtures/provider_attachment.rs"]
#[allow(dead_code, unused_imports)]
mod audio_provider;
#[path = "../fixtures/album_complete_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
#[allow(unused_imports)]
use hito_core::test_support::support;

use hito_core::{AlbumTrackAudio, Credentials, FindArtistTrackAudio, UpdateAutomaticLookups};
use library::Album;
use serde_json::{Value, json};
use std::time::Duration;

const BONUS_OCCURRENCE: &str = "occurrence:album/2/2";
const PREGAP_OCCURRENCE: &str = "occurrence:album/1/0";
const BONUS_TITLE: &str = "Bonus catalog title";
const SOURCE: &str = "audio-provider/bonus-source";

fn request(occurrence: &str) -> FindArtistTrackAudio {
    FindArtistTrackAudio {
        release_group_id: provider::GROUP_ID.into(),
        edition_id: provider::EDITION_ID.into(),
        occurrence_id: occurrence.into(),
    }
}

async fn find(album: &Album, occurrence: &str) -> hito_core::Result<AlbumTrackAudio> {
    album
        .hito
        .artist_catalog
        .find_artist_track_audio_until(
            &album.actor,
            &album.first,
            &album.association.id,
            request(occurrence),
            std::future::pending(),
        )
        .await
}

fn candidate(id: &str, title: &str, artist: &str, duration_ms: u64, source: &str) -> [Value; 3] {
    let detail = json!({
        "id": id, "title": title, "artist": artist, "duration_ms": duration_ms,
        "release": null, "disc_number": null, "track_number": null, "revision": "audio-r1"
    });
    let metadata = json!({
        "recording": detail, "musical_title": title,
        "album_title": provider::RELEASE_TITLE, "source_ids": [source]
    });
    let sources = json!({"items": [{"id": source, "label": "Owned audio", "ready": true}]});
    [detail, metadata, sources]
}

async fn offer(
    audio: &audio_provider::ProviderServer,
    id: &str,
    title: &str,
    artist: &str,
    duration_ms: u64,
) {
    let [detail, metadata, sources] = candidate(id, title, artist, duration_ms, SOURCE);
    audio
        .reply(
            "v1/search",
            json!({"items": [detail.clone()], "next_cursor": null}),
        )
        .await;
    audio.reply("v1/recordings/detail", detail).await;
    audio.reply("v1/recordings/metadata", metadata).await;
    audio.reply("v1/sources", sources).await;
}

async fn connect_audio(
    album: &Album,
    audio: &audio_provider::ProviderServer,
) -> hito_core::PluginInstallation {
    audio_provider::connect(
        &album.hito,
        &album.actor,
        audio,
        "org.hito.audio-fixture",
        &["search", "sources", "metadata"],
    )
    .await
}

#[tokio::test]
async fn a_catalog_track_of_a_reviewed_artist_is_matched_and_saved_without_a_library_album() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(
        &audio,
        "audio-provider/bonus",
        BONUS_TITLE,
        provider::CREDIT,
        1000,
    )
    .await;
    let total = album.total().await;

    let AlbumTrackAudio::Saved { recording } = find(&album, BONUS_OCCURRENCE).await.unwrap() else {
        panic!("An exact title, credit and duration must match approved audio");
    };
    assert_eq!(album.total().await, total + 1);
    assert_eq!(recording.title, BONUS_TITLE);
    let sources = album
        .hito
        .catalog
        .sources(&album.actor, &recording.id)
        .await
        .unwrap();
    assert!(sources.iter().any(|source| source.ready));
    assert!(audio.requests("v1/audio").await.is_empty());

    let AlbumTrackAudio::Saved { recording: again } = find(&album, BONUS_OCCURRENCE).await.unwrap()
    else {
        panic!("A repeated search must succeed");
    };
    assert_eq!(again.id, recording.id);
    assert_eq!(album.total().await, total + 1);
    assert_eq!(catalog.unexpected(), 0);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn an_uncertain_candidate_is_no_match_and_nothing_is_saved() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(
        &audio,
        "audio-provider/other",
        "Different catalog title",
        provider::CREDIT,
        1000,
    )
    .await;
    let before = album.library().await;

    assert!(matches!(
        find(&album, BONUS_OCCURRENCE).await.unwrap(),
        AlbumTrackAudio::NoMatch
    ));
    assert_eq!(album.library().await, before);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn without_a_connection_an_outage_or_a_cancellation_nothing_is_saved() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    assert!(matches!(
        find(&album, BONUS_OCCURRENCE).await.unwrap(),
        AlbumTrackAudio::NoProviders
    ));
    connect_audio(&album, &audio).await;
    offer(
        &audio,
        "audio-provider/bonus",
        BONUS_TITLE,
        provider::CREDIT,
        1000,
    )
    .await;
    let before = album.library().await;
    audio.offline().await;
    assert!(matches!(
        find(&album, BONUS_OCCURRENCE).await.unwrap(),
        AlbumTrackAudio::Unavailable
    ));
    audio.online().await;
    let cancelled = tokio::time::timeout(
        Duration::from_secs(5),
        album.hito.artist_catalog.find_artist_track_audio_until(
            &album.actor,
            &album.first,
            &album.association.id,
            request(BONUS_OCCURRENCE),
            std::future::ready(()),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(cancelled, AlbumTrackAudio::Cancelled));
    assert_eq!(album.library().await, before);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn only_audio_occurrences_can_be_searched_and_another_account_is_refused() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(
        &audio,
        "audio-provider/bonus",
        BONUS_TITLE,
        provider::CREDIT,
        1000,
    )
    .await;
    for occurrence in [PREGAP_OCCURRENCE, "occurrence:album/9/9"] {
        assert!(matches!(
            find(&album, occurrence).await,
            Err(hito_core::Error::NotFound)
        ));
    }
    let credentials = Credentials {
        username: "stranger".into(),
        password: "artist track stranger password".into(),
    };
    album
        .hito
        .identity
        .create_user(
            &album.actor,
            Credentials {
                username: credentials.username.clone(),
                password: credentials.password.clone(),
            },
        )
        .await
        .unwrap();
    let session = album.hito.identity.login(credentials).await.unwrap();
    let stranger = album
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(
        album
            .hito
            .artist_catalog
            .find_artist_track_audio_until(
                &stranger,
                &album.first,
                &album.association.id,
                request(BONUS_OCCURRENCE),
                std::future::pending(),
            )
            .await
            .is_err()
    );
    assert!(audio.requests("v1/search").await.is_empty());
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn turning_off_automatic_service_search_stops_a_catalog_track_search_before_any_provider_call()
 {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(
        &audio,
        "audio-provider/bonus",
        BONUS_TITLE,
        provider::CREDIT,
        1000,
    )
    .await;
    let before = album.library().await;
    let search_choice = |on: bool| UpdateAutomaticLookups {
        automatic_artist_lookup: None,
        automatic_artwork: None,
        automatic_service_search: Some(on),
    };
    album
        .hito
        .artist_catalog
        .set_automatic_lookups(&album.actor, search_choice(false))
        .await
        .unwrap();
    assert!(matches!(
        find(&album, BONUS_OCCURRENCE).await.unwrap(),
        AlbumTrackAudio::SearchOff
    ));
    assert!(audio.requests("v1/search").await.is_empty());
    assert_eq!(album.library().await, before);
    album
        .hito
        .artist_catalog
        .set_automatic_lookups(&album.actor, search_choice(true))
        .await
        .unwrap();
    assert!(matches!(
        find(&album, BONUS_OCCURRENCE).await.unwrap(),
        AlbumTrackAudio::Saved { .. }
    ));
    audio.shutdown().await;
    assert!(catalog.finish().await);
}
