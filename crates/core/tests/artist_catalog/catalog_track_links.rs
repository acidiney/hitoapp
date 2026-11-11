#[path = "../fixtures/provider_attachment.rs"]
#[allow(dead_code, unused_imports)]
mod audio_provider;
#[path = "../fixtures/album_complete_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
#[allow(unused_imports)]
use hito_core::test_support::support;

use hito_core::{
    AlbumEditionTrackListAvailability, AlbumTrackAudio, CatalogTrackMatch, Credentials,
    EditionTrackListAvailability, FindAlbumTrackAudio, FindArtistTrackAudio, ReadEditionTrackList,
};
use library::Album;
use serde_json::{Value, json};
use std::time::Duration;

const BONUS_OCCURRENCE: &str = "occurrence:album/2/2";
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

async fn find_on_library_album(
    album: &Album,
    occurrence: &str,
) -> hito_core::Result<AlbumTrackAudio> {
    album
        .hito
        .artist_catalog
        .find_album_track_audio_until(
            &album.actor,
            &album.release_id,
            FindAlbumTrackAudio {
                recording_id: album.first.clone(),
                association_id: album.association.id.clone(),
                release_group_id: provider::GROUP_ID.into(),
                edition_id: provider::EDITION_ID.into(),
                occurrence_id: occurrence.into(),
            },
            std::future::pending(),
        )
        .await
}

async fn linked(album: &Album) -> Vec<CatalogTrackMatch> {
    let read = album
        .hito
        .artist_catalog
        .recording_artist_edition_track_list_until(
            &album.actor,
            &album.first,
            &album.association.id,
            ReadEditionTrackList {
                release_group_id: provider::GROUP_ID.into(),
                edition_id: provider::EDITION_ID.into(),
            },
            std::future::pending(),
        )
        .await
        .unwrap();
    let EditionTrackListAvailability::Available { linked, .. } = read else {
        panic!("The edition must be readable");
    };
    linked
}

fn offer_body(id: &str, title: &str, artist: &str, duration_ms: u64) -> [Value; 3] {
    let detail = json!({
        "id": id, "title": title, "artist": artist, "duration_ms": duration_ms,
        "release": null, "disc_number": null, "track_number": null, "revision": "audio-r1"
    });
    let metadata = json!({
        "recording": detail, "musical_title": title,
        "album_title": provider::RELEASE_TITLE, "source_ids": [SOURCE]
    });
    let sources = json!({"items": [{"id": SOURCE, "label": "Owned audio", "ready": true}]});
    [detail, metadata, sources]
}

async fn offer(audio: &audio_provider::ProviderServer, id: &str, title: &str, duration_ms: u64) {
    let [detail, metadata, sources] = offer_body(id, title, provider::CREDIT, duration_ms);
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

async fn connect_audio(album: &Album, audio: &audio_provider::ProviderServer) {
    audio_provider::connect(
        &album.hito,
        &album.actor,
        audio,
        "org.hito.audio-fixture",
        &["search", "sources", "metadata"],
    )
    .await;
}

#[tokio::test]
async fn a_found_recording_becomes_the_durable_link_of_its_catalog_row() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(&audio, "audio-provider/bonus", BONUS_TITLE, 1000).await;
    assert!(linked(&album).await.is_empty());

    let AlbumTrackAudio::Saved { recording } = find(&album, BONUS_OCCURRENCE).await.unwrap() else {
        panic!("An exact match must be saved");
    };

    let links = linked(&album).await;
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].occurrence_id, BONUS_OCCURRENCE);
    assert_eq!(links[0].recording.id, recording.id);
    assert_eq!(links[0].recording.title, BONUS_TITLE);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn a_failed_or_cancelled_search_leaves_the_row_unlinked() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(
        &audio,
        "audio-provider/other",
        "Different catalog title",
        1000,
    )
    .await;
    audio
        .reply(
            "v1/sources",
            json!({"items": [{"id": SOURCE, "label": "Owned audio", "ready": false}]}),
        )
        .await;
    assert!(matches!(
        find(&album, BONUS_OCCURRENCE).await.unwrap(),
        AlbumTrackAudio::NoMatch
    ));
    offer(&audio, "audio-provider/bonus", BONUS_TITLE, 1000).await;
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

    assert!(linked(&album).await.is_empty());
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn replaying_a_linked_row_returns_the_linked_recording_without_a_new_search() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(&audio, "audio-provider/bonus", BONUS_TITLE, 1000).await;
    let AlbumTrackAudio::Saved { recording } = find(&album, BONUS_OCCURRENCE).await.unwrap() else {
        panic!("An exact match must be saved");
    };
    let searches = audio.requests("v1/search").await.len();
    let total = album.total().await;

    for _ in 0..2 {
        let AlbumTrackAudio::Saved { recording: again } =
            find(&album, BONUS_OCCURRENCE).await.unwrap()
        else {
            panic!("A linked row must answer with its recording");
        };
        assert_eq!(again.id, recording.id);
    }

    assert_eq!(audio.requests("v1/search").await.len(), searches);
    assert_eq!(album.total().await, total);
    assert_eq!(linked(&album).await.len(), 1);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn a_removed_recording_unlinks_its_row() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(&audio, "audio-provider/bonus", BONUS_TITLE, 1000).await;
    let AlbumTrackAudio::Saved { recording } = find(&album, BONUS_OCCURRENCE).await.unwrap() else {
        panic!("An exact match must be saved");
    };
    assert_eq!(linked(&album).await.len(), 1);

    album
        .sql("DELETE FROM recordings WHERE id = ?", &[&recording.id])
        .await;

    assert!(linked(&album).await.is_empty());
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn another_accounts_link_is_never_shown() {
    let mut catalog = AlbumProvider::start().await;
    let album = Album::prepare(&catalog).await;
    let credentials = Credentials {
        username: "link stranger".into(),
        password: "catalog link stranger password".into(),
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
    album
        .sql(
            "INSERT INTO catalog_track_links(user_id, provider_kind, plugin_installation_id, edition_id, occurrence_id, recording_id)
            VALUES(?, 'http_plugin', ?, ?, ?, ?)",
            &[
                &stranger.user().id,
                &album.installation.id,
                provider::EDITION_ID,
                BONUS_OCCURRENCE,
                &album.extra,
            ],
        )
        .await;

    assert!(linked(&album).await.is_empty());
    assert!(
        album
            .hito
            .artist_catalog
            .recording_artist_edition_track_list_until(
                &stranger,
                &album.first,
                &album.association.id,
                ReadEditionTrackList {
                    release_group_id: provider::GROUP_ID.into(),
                    edition_id: provider::EDITION_ID.into(),
                },
                std::future::pending(),
            )
            .await
            .is_err()
    );
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn a_library_album_comparison_shows_the_link_as_a_match() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(&audio, "audio-provider/bonus", BONUS_TITLE, 1000).await;
    let AlbumTrackAudio::Saved { recording } = find_on_library_album(&album, BONUS_OCCURRENCE)
        .await
        .unwrap()
    else {
        panic!("An exact match must be saved");
    };

    let AlbumEditionTrackListAvailability::Available { matches, .. } =
        album.read(provider::EDITION_ID).await.unwrap()
    else {
        panic!("The comparison must be readable");
    };

    let linked: Vec<&CatalogTrackMatch> = matches
        .iter()
        .filter(|found| found.occurrence_id == BONUS_OCCURRENCE)
        .collect();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].recording.id, recording.id);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn deleting_an_account_removes_only_its_links() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(&audio, "audio-provider/bonus", BONUS_TITLE, 1000).await;
    find(&album, BONUS_OCCURRENCE).await.unwrap();
    let credentials = Credentials {
        username: "doomed link account".into(),
        password: "catalog link doomed password".into(),
    };
    let doomed = album
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
    album
        .sql(
            "INSERT INTO catalog_track_links(user_id, provider_kind, plugin_installation_id, edition_id, occurrence_id, recording_id)
            VALUES(?, 'http_plugin', ?, ?, ?, ?)",
            &[
                &doomed.id,
                &album.installation.id,
                provider::EDITION_ID,
                BONUS_OCCURRENCE,
                &album.extra,
            ],
        )
        .await;
    let count = "SELECT count(*) FROM catalog_track_links";

    album
        .hito
        .identity
        .delete_account(
            &album.actor,
            &doomed.id,
            hito_core::DeleteAccount {
                username: credentials.username,
            },
        )
        .await
        .unwrap();

    assert_eq!(album.count(count, &[]).await, 1);
    assert_eq!(linked(&album).await.len(), 1);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}
