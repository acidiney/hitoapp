use super::{
    Server,
    public_fixture::{Fixture, PLAYLIST_ID, owned_entry},
};
use crate::{Config, router};
use hito_plugin_sdk::{PlaylistItemsRequest, PlaylistPage, ProviderPlaylist, ProviderPlaylistItem};
use serde_json::{Value, json};
use std::time::Duration;

fn credited(entry: &mut Value, artists: Vec<Value>) {
    entry["itemV2"]["data"]["artists"] = json!({"items": artists});
}

async fn recording_pages(entry: Value) -> (Vec<String>, Option<String>) {
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
    (recording.artist_urls.clone(), recording.isrc.clone())
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
