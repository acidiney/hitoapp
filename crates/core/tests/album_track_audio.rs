#[path = "fixtures/provider_attachment.rs"]
#[allow(dead_code, unused_imports)]
mod audio_provider;
#[path = "fixtures/album_complete_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
#[allow(unused_imports)]
use hito_core::test_support::support;

use hito_core::{AdoptProviderRecording, AlbumTrackAudio, Credentials, FindAlbumTrackAudio};
use library::Album;
use serde_json::{Value, json};
use std::time::Duration;

const BONUS_OCCURRENCE: &str = "occurrence:album/2/2";
const PREGAP_OCCURRENCE: &str = "occurrence:album/1/0";
const BONUS_TITLE: &str = "Bonus catalog title";
const SOURCE: &str = "audio-provider/bonus-source";

fn request(album: &Album, occurrence: &str) -> FindAlbumTrackAudio {
    FindAlbumTrackAudio {
        recording_id: album.first.clone(),
        association_id: album.association.id.clone(),
        release_group_id: provider::GROUP_ID.into(),
        edition_id: provider::EDITION_ID.into(),
        occurrence_id: occurrence.into(),
    }
}

async fn find(album: &Album, occurrence: &str) -> hito_core::Result<AlbumTrackAudio> {
    album
        .hito
        .catalog
        .find_album_track_audio_until(
            &album.actor,
            &album.release_id,
            request(album, occurrence),
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

async fn nothing_requested(audio: &audio_provider::ProviderServer) {
    for path in ["v1/audio"] {
        assert!(
            audio.requests(path).await.is_empty(),
            "{path} must not be called"
        );
    }
}

#[tokio::test]
async fn a_catalog_only_track_is_matched_to_approved_audio_and_saved_only_by_adoption() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    let installation = connect_audio(&album, &audio).await;
    offer(
        &audio,
        "audio-provider/bonus",
        BONUS_TITLE,
        provider::CREDIT,
        1000,
    )
    .await;
    let before = album.library().await;
    let details = catalog.requests_to("v1/recordings/detail").len();

    let found = find(&album, BONUS_OCCURRENCE).await.unwrap();
    let AlbumTrackAudio::Found {
        installation_id,
        installation_revision,
        provider_recording_id,
    } = found
    else {
        panic!("An exact title, credit and duration must match approved audio");
    };
    assert_eq!(installation_id, installation.id);
    assert_eq!(installation_revision, installation.revision);
    assert_eq!(provider_recording_id, "audio-provider/bonus");
    assert_eq!(
        album.library().await,
        before,
        "Matching saves nothing: recordings, releases and personal edits are unchanged"
    );
    assert_eq!(
        audio.requests("v1/search").await[0].1,
        json!({"query": format!("{} {}", provider::CREDIT, BONUS_TITLE), "cursor": null, "limit": 20}),
        "The search uses the catalog's own title and credit"
    );
    assert_eq!(catalog.requests_to("v1/recordings/detail").len(), details);
    nothing_requested(&audio).await;

    let saved = album
        .hito
        .catalog
        .adopt_provider(
            &album.actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id,
            },
        )
        .await
        .unwrap();
    let sources = album
        .hito
        .catalog
        .sources(&album.actor, &saved.id)
        .await
        .unwrap();
    assert!(
        sources.iter().any(|source| source.ready),
        "The adopted recording is playable through the approved provider"
    );
    assert_eq!(catalog.unexpected(), 0);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn uncertain_candidates_are_never_returned() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    let before = album.library().await;

    let wrong = [
        (
            "another title",
            "Different catalog title",
            provider::CREDIT,
            1000,
        ),
        (
            "another performer",
            BONUS_TITLE,
            "Different performer",
            1000,
        ),
        ("another length", BONUS_TITLE, provider::CREDIT, 90_000),
        (
            "a live version",
            "Bonus catalog title (Live)",
            provider::CREDIT,
            1000,
        ),
    ];
    for (label, title, artist, duration) in wrong {
        offer(&audio, "audio-provider/uncertain", title, artist, duration).await;
        assert!(
            matches!(
                find(&album, BONUS_OCCURRENCE).await.unwrap(),
                AlbumTrackAudio::NoMatch
            ),
            "{label} must not match"
        );
    }
    assert_eq!(album.library().await, before);
    nothing_requested(&audio).await;
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn provider_outages_and_missing_connections_are_honest_and_retryable() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    assert!(
        matches!(
            find(&album, BONUS_OCCURRENCE).await.unwrap(),
            AlbumTrackAudio::NoProviders
        ),
        "Without an approved search and source connection there is nothing to ask"
    );
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
    assert!(matches!(
        find(&album, BONUS_OCCURRENCE).await.unwrap(),
        AlbumTrackAudio::Found { .. }
    ));

    catalog.set_outage(true);
    assert!(matches!(
        find(&album, BONUS_OCCURRENCE).await.unwrap(),
        AlbumTrackAudio::Unavailable
    ));
    catalog.set_outage(false);
    assert_eq!(album.library().await, before, "The album stays as it was");
    assert!(
        album
            .hito
            .catalog
            .sources(&album.actor, &album.first)
            .await
            .is_ok()
    );
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn only_audio_occurrences_of_the_chosen_edition_can_be_searched_and_cancellation_stops_the_search()
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

    for occurrence in [PREGAP_OCCURRENCE, "occurrence:album/9/9"] {
        assert!(matches!(
            find(&album, occurrence).await,
            Err(hito_core::Error::NotFound)
        ));
    }
    let cancelled = tokio::time::timeout(
        Duration::from_secs(5),
        album.hito.catalog.find_album_track_audio_until(
            &album.actor,
            &album.release_id,
            request(&album, BONUS_OCCURRENCE),
            std::future::ready(()),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(cancelled, AlbumTrackAudio::Cancelled));
    assert!(audio.requests("v1/search").await.is_empty());

    let credentials = Credentials {
        username: "stranger".into(),
        password: "album stranger password".into(),
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
    assert!(matches!(
        album
            .hito
            .catalog
            .find_album_track_audio_until(
                &stranger,
                &album.release_id,
                request(&album, BONUS_OCCURRENCE),
                std::future::pending(),
            )
            .await,
        Err(hito_core::Error::NotFound)
    ));
    assert!(audio.requests("v1/search").await.is_empty());
    audio.shutdown().await;
    assert!(catalog.finish().await);
}
