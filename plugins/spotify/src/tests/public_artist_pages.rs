use super::{
    Server,
    public_fixture::{Fixture, PLAYLIST_ID, owned_entry},
};
use crate::{Config, router};
use hito_plugin_sdk::{
    CreditRole, PlaylistItemsRequest, PlaylistPage, ProviderPlaylist, ProviderPlaylistItem,
    ProviderRecording,
};
use serde_json::{Value, json};
use std::time::Duration;

fn credited(entry: &mut Value, artists: Vec<Value>) {
    entry["itemV2"]["data"]["artists"] = json!({"items": artists});
}

async fn recording_of(entry: Value) -> ProviderRecording {
    let fixture = Fixture::with_entries(vec![entry]).await;
    let plugin = Server::start(
        router(Config {
            public_base: fixture.server.url.clone(),
            query_endpoint: fixture.server.url.join("pathfinder/v2/query").unwrap(),
        })
        .unwrap(),
    )
    .await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let playlist = client
        .post(plugin.url.join("v1/playlists/detail").unwrap())
        .json(&json!({"kind": "canonical", "playlist_id": PLAYLIST_ID}))
        .send()
        .await
        .unwrap()
        .json::<ProviderPlaylist>()
        .await
        .unwrap();
    let request = PlaylistItemsRequest {
        playlist_id: playlist.id.clone(),
        snapshot: playlist.snapshot.clone(),
        cursor: None,
        limit: 100,
    };
    let page = client
        .post(plugin.url.join("v1/playlists/items").unwrap())
        .json(&request)
        .send()
        .await
        .unwrap()
        .json::<PlaylistPage>()
        .await
        .unwrap();
    page.validate(request.limit).unwrap();
    let ProviderPlaylistItem::Recording { recording } = &page.items[0].item else {
        panic!("Expected the owned recording");
    };
    (**recording).clone()
}

async fn recording_pages(entry: Value) -> (Vec<String>, Option<String>) {
    let recording = recording_of(entry).await;
    (recording.artist_urls, recording.isrc)
}

fn artist(id: &str) -> Value {
    json!({"uri": format!("spotify:artist:{id}"), "profile": {"name": "Owned fixture artist"}})
}

#[tokio::test]
async fn public_items_publish_the_credited_artist_pages_and_never_invent_an_isrc() {
    let mut one = owned_entry(0);
    credited(&mut one, vec![artist("0000000000000000000001")]);
    let (urls, isrc) = recording_pages(one).await;
    assert_eq!(
        urls,
        ["https://open.spotify.com/artist/0000000000000000000001"]
    );
    assert!(
        isrc.is_none(),
        "The public playlist response states no ISRC"
    );

    let mut two = owned_entry(0);
    credited(
        &mut two,
        vec![
            artist("0000000000000000000001"),
            artist("0000000000000000000002"),
        ],
    );
    assert_eq!(recording_pages(two).await.0.len(), 2);
}

#[tokio::test]
async fn a_credit_with_an_unidentified_artist_publishes_no_artist_pages_but_still_imports() {
    for missing in [
        json!({"profile": {"name": "No uri"}}),
        json!({"uri": "spotify:artist:1", "profile": {"name": "Short id"}}),
        json!({"uri": "spotify:user:0000000000000000000001", "profile": {"name": "Wrong namespace"}}),
        json!({"uri": 17, "profile": {"name": "Not text"}}),
    ] {
        let mut entry = owned_entry(0);
        credited(&mut entry, vec![artist("0000000000000000000001"), missing]);
        let (urls, _) = recording_pages(entry).await;
        assert!(urls.is_empty(), "A partial list would hide a performer");
    }
}

fn named(name: &str, id: &str) -> Value {
    json!({"uri": format!("spotify:artist:{id}"), "profile": {"name": name}})
}

#[tokio::test]
async fn public_items_state_each_credited_artist_in_order_with_its_spotify_identity() {
    let mut entry = owned_entry(0);
    credited(
        &mut entry,
        vec![
            named("Nova Quartet", "0000000000000000000001"),
            named("Harbor Choir", "0000000000000000000002"),
        ],
    );
    let recording = recording_of(entry).await;
    assert_eq!(recording.artist, "Nova Quartet, Harbor Choir");
    let listed: Vec<_> = recording
        .artists
        .iter()
        .map(|artist| {
            (
                artist.name.as_str(),
                artist.role,
                artist.join.as_deref(),
                artist.provider_artist_id.as_deref(),
                artist.page_url.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (
                "Nova Quartet",
                CreditRole::Main,
                Some(", "),
                Some("0000000000000000000001"),
                Some("https://open.spotify.com/artist/0000000000000000000001")
            ),
            (
                "Harbor Choir",
                CreditRole::Main,
                None,
                Some("0000000000000000000002"),
                Some("https://open.spotify.com/artist/0000000000000000000002")
            )
        ]
    );
    assert_eq!(recording.artist_urls.len(), 2, "The older page list stays");
}

#[tokio::test]
async fn an_artist_named_by_the_feature_clause_of_the_title_is_featured() {
    for (title, featured) in [
        ("Harbor Lights (feat. Harbor Choir)", true),
        ("Harbor Lights [ft. harbor choir]", true),
        ("Harbor Lights (Featuring Harbor Choir)", true),
        ("Harbor Lights - feat. Harbor Choir", true),
        ("Harbor Lights (with Harbor Choir)", false),
        ("Harbor Lights (feat. Someone Else)", false),
        ("Harbor Choir (feat. Harbor Choir)", true),
    ] {
        let mut entry = owned_entry(0);
        entry["itemV2"]["data"]["name"] = json!(title);
        credited(
            &mut entry,
            vec![
                named("Nova Quartet", "0000000000000000000001"),
                named("Harbor Choir", "0000000000000000000002"),
            ],
        );
        let roles: Vec<CreditRole> = recording_of(entry)
            .await
            .artists
            .iter()
            .map(|artist| artist.role)
            .collect();
        let expected = if featured {
            [CreditRole::Main, CreditRole::Featured]
        } else {
            [CreditRole::Main, CreditRole::Main]
        };
        assert_eq!(roles, expected, "{title}");
    }
    let mut only = owned_entry(0);
    only["itemV2"]["data"]["name"] = json!("Song (feat. Solo)");
    credited(&mut only, vec![named("Solo", "0000000000000000000001")]);
    assert_eq!(
        recording_of(only).await.artists[0].role,
        CreditRole::Main,
        "A credit never loses its last main artist"
    );
}

#[tokio::test]
async fn an_artist_without_a_usable_identity_is_still_listed_by_name() {
    let mut entry = owned_entry(0);
    credited(
        &mut entry,
        vec![
            named("Nova Quartet", "0000000000000000000001"),
            json!({"profile": {"name": "No uri"}}),
        ],
    );
    let recording = recording_of(entry).await;
    assert_eq!(recording.artists.len(), 2);
    assert_eq!(recording.artists[1].name, "No uri");
    assert!(recording.artists[1].provider_artist_id.is_none());
    assert!(recording.artists[1].page_url.is_none());
    assert!(recording.artist_urls.is_empty());
}

#[tokio::test]
async fn a_track_with_more_credited_artists_than_page_links_still_imports_with_every_credit() {
    let mut entry = owned_entry(0);
    let artists = (1..=10)
        .map(|number| {
            named(
                &format!("Owned collaborator {number}"),
                &format!("{number:022}"),
            )
        })
        .collect();
    credited(&mut entry, artists);
    let recording = recording_of(entry).await;
    assert_eq!(recording.artists.len(), 10);
    assert!(recording.artist_urls.is_empty());
    assert!(
        recording
            .artists
            .iter()
            .all(|artist| artist.page_url.is_some())
    );
}
