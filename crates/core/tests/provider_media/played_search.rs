use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, Actor, AdoptProviderRecording, Browse, Credentials, InstallPlugin, OfferPluginService,
    Search, Services,
};
use serde_json::{Value, json};
use uuid::Uuid;

const ALBUM: [(&str, &str); 2] = [("opening", "Quiet Harbour"), ("closing", "Last Lantern")];

type Found = (Vec<String>, Vec<(String, i64)>, Vec<String>);

async fn connect(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
) -> hito_core::PluginInstallation {
    server
        .reply(
            "v1/manifest",
            json!({"id":"org.hito.played-fixture","name":"Played fixture","protocol_version":1,
                "capabilities":["search","sources","metadata"]}),
        )
        .await;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Played fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}

fn video(id: &str, title: &str) -> Value {
    json!({
        "id": id, "title": title, "artist": "Lumen Tide", "duration_ms": 1_000,
        "artists": [{"name": "Lumen Tide", "role": "main"}]
    })
}

async fn play_album_from_service(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
    album: Option<&str>,
) -> Vec<String> {
    let installed = connect(hito, actor, server).await;
    play_album(hito, actor, server, &installed, album).await
}

async fn play_album(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
    installed: &hito_core::PluginInstallation,
    album: Option<&str>,
) -> Vec<String> {
    let mut played = Vec::new();
    for (id, title) in ALBUM {
        let detail = video(id, title);
        server
            .recording_reply("v1/recordings/detail", id, detail.clone())
            .await;
        server
            .recording_reply(
                "v1/sources",
                id,
                json!({"items": [{"id": format!("{id}/audio"), "label": "Owned audio", "ready": true}]}),
            )
            .await;
        if let Some(album) = album {
            server
                .recording_reply(
                    "v1/recordings/metadata",
                    id,
                    json!({"recording": detail, "musical_title": title, "album_title": album,
                        "source_ids": [format!("{id}/audio")]}),
                )
                .await;
        }
        let recording = hito
            .catalog
            .adopt_provider(
                actor,
                &installed.id,
                AdoptProviderRecording {
                    installation_revision: installed.revision,
                    provider_recording_id: id.into(),
                },
            )
            .await
            .unwrap();
        assert!(recording.available);
        support::synchronize(
            hito,
            actor,
            vec![Action::ProgressSet {
                recording_id: Uuid::parse_str(&recording.id).unwrap(),
                position_ms: 1_000,
                speed: 1.0,
            }],
        )
        .await;
        played.push(recording.id);
    }
    played.sort();
    played
}

async fn found(hito: &Services, actor: &Actor, q: &str) -> Found {
    let tracks = hito
        .catalog
        .recordings(
            actor,
            Search {
                q: Some(q.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let albums = hito
        .catalog
        .releases(
            actor,
            Browse {
                q: Some(q.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let artists = hito
        .catalog
        .artists(
            actor,
            Browse {
                q: Some(q.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let mut tracks: Vec<String> = tracks.items.into_iter().map(|track| track.id).collect();
    tracks.sort();
    (
        tracks,
        albums
            .items
            .into_iter()
            .map(|album| (album.title, album.track_count))
            .collect(),
        artists
            .items
            .into_iter()
            .map(|artist| artist.name)
            .collect(),
    )
}

#[tokio::test]
async fn an_album_a_service_names_only_in_musical_metadata_is_found_by_title_album_and_artist() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("played-search").await;
    let played = play_album_from_service(&hito, &actor, &server, Some("Northern Lanterns")).await;
    let album = vec![("Northern Lanterns".to_owned(), 2)];

    let (tracks, albums, _) = found(&hito, &actor, "quiet harb").await;
    assert_eq!(tracks.len(), 1);
    assert!(albums.is_empty());
    assert_eq!(found(&hito, &actor, "northern lantern").await.1, album);
    assert_eq!(
        found(&hito, &actor, "lumen").await,
        (played, album, vec!["Lumen Tide".to_owned()])
    );
}

#[tokio::test]
async fn a_service_without_musical_metadata_still_saves_the_played_track_without_an_album() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("played-search-no-album").await;
    let played = play_album_from_service(&hito, &actor, &server, None).await;
    assert_eq!(
        found(&hito, &actor, "lumen").await,
        (played, vec![], vec!["Lumen Tide".to_owned()])
    );
}

#[tokio::test]
async fn playing_an_earlier_track_again_names_the_album_the_service_now_reports() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("played-search-again").await;
    let installed = connect(&hito, &actor, &server).await;
    let before = play_album(&hito, &actor, &server, &installed, None).await;
    assert!(found(&hito, &actor, "northern").await.1.is_empty());
    let after = play_album(
        &hito,
        &actor,
        &server,
        &installed,
        Some("Northern Lanterns"),
    )
    .await;
    assert_eq!(after, before);
    assert_eq!(
        found(&hito, &actor, "northern").await.1,
        vec![("Northern Lanterns".to_owned(), 2)]
    );
}

#[tokio::test]
async fn another_account_does_not_find_what_this_account_played() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, admin, library, _music) = support::library("played-search-private").await;
    play_album_from_service(&hito, &admin, &server, Some("Northern Lanterns")).await;
    let credentials = || Credentials {
        username: "member".into(),
        password: "played search member password".into(),
    };
    let member = hito
        .identity
        .create_user(&admin, credentials())
        .await
        .unwrap();
    hito.identity
        .set_library_access(&admin, &library, &member.id, true)
        .await
        .unwrap();
    let session = hito.identity.login(credentials()).await.unwrap();
    let member = hito.identity.authenticate(&session.token).await.unwrap();
    for q in ["quiet harbour", "northern lanterns", "lumen tide"] {
        assert_eq!(found(&hito, &member, q).await, (vec![], vec![], vec![]));
    }
}
