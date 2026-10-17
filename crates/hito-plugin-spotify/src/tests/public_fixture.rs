use super::Server;
use axum::{
    Json, Router,
    http::{HeaderMap, Uri},
    response::Html,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const PLAYLIST_ID: &str = "0000000000000000000001";
const QUERY_HASH: &str = "c685ca91fb0019c87ff1225f84e1d85b26cd631fa6187f3f0900da182cb0953d";

pub(super) struct Fixture {
    pub server: Server,
    pub calls: Arc<Mutex<Vec<String>>>,
    pub tail_changed: Arc<AtomicBool>,
}

impl Fixture {
    pub async fn start() -> Self {
        Self::with_entries((0..101).map(owned_entry).collect()).await
    }

    pub async fn with_entries(entries: Vec<Value>) -> Self {
        Self::with_routes(entries, Router::new()).await
    }

    pub async fn with_routes(entries: Vec<Value>, routes: Router) -> Self {
        let entries = Arc::new(entries);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let tail_changed = Arc::new(AtomicBool::new(false));
        let embed_calls = calls.clone();
        let query_calls = calls.clone();
        let changed = tail_changed.clone();
        let server = Server::start(
            Router::new()
                .route(
                    "/embed/playlist/0000000000000000000001",
                    get(move |headers: HeaderMap, uri: Uri| {
                        let calls = embed_calls.clone();
                        async move {
                            assert_eq!(uri.path_and_query().unwrap().as_str(), format!("/embed/playlist/{PLAYLIST_ID}"));
                            assert!(headers.get("authorization").is_none());
                            assert!(headers.get("cookie").is_none());
                            calls.lock().unwrap().push("embed".to_owned());
                            bootstrap_html("owned-anonymous-bootstrap-token")
                        }
                    }),
                )
                .route(
                    "/pathfinder/v2/query",
                    post(move |headers: HeaderMap, uri: Uri, Json(body): Json<Value>| {
                        let calls = query_calls.clone();
                        let changed = changed.clone();
                        let entries = entries.clone();
                        async move {
                            assert_eq!(uri.path_and_query().unwrap().as_str(), "/pathfinder/v2/query");
                            assert!(headers.get("authorization").is_some_and(|value| value == "Bearer owned-anonymous-bootstrap-token"));
                            assert!(headers.get("cookie").is_none());
                            let offset = body["variables"]["offset"].as_u64().unwrap();
                            assert!(offset == 0 || offset == 100);
                            assert_eq!(body, json!({
                                "operationName": "queryPlaylist",
                                "variables": {
                                    "uri": format!("spotify:playlist:{PLAYLIST_ID}"),
                                    "limit": 100,
                                    "offset": offset,
                                    "includeItemV3": false
                                },
                                "extensions": {"persistedQuery": {
                                    "version": 1,
                                    "sha256Hash": QUERY_HASH
                                }}
                            }));
                            calls.lock().unwrap().push(format!("query:{offset}"));
                            let total = entries.len() as u64;
                            let end = (offset + 100).min(total);
                            let items: Vec<_> = (offset..end)
                                .map(|position| {
                                    let mut entry = entries[position as usize].clone();
                                    if position == 100 && changed.load(Ordering::SeqCst) {
                                        entry["itemV2"]["data"]["uri"] = json!(format!("spotify:track:{:022}", 999));
                                        entry["itemV2"]["data"]["name"] = json!("Owned revised tail recording");
                                        entry["itemV2"]["data"]["trackNumber"] = json!(999);
                                    }
                                    entry
                                })
                                .collect();
                            Json(json!({
                                "data": {"playlistV2": {
                                    "__typename": "Playlist",
                                    "uri": format!("spotify:playlist:{PLAYLIST_ID}"),
                                    "name": "Owned public playlist fixture",
                                    "content": {
                                        "items": items,
                                        "totalCount": total,
                                        "pagingInfo": {"nextOffset": if end < total { Some(end) } else { None }}
                                    }
                                }}
                            }))
                        }
                    }),
                )
                .merge(routes),
        )
        .await;
        Self {
            server,
            calls,
            tail_changed,
        }
    }
}

pub(super) fn bootstrap_html(token: &str) -> Html<String> {
    let expiration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 600_000;
    let bootstrap = json!({
        "props": {"pageProps": {"state": {"settings": {"session": {
            "accessToken": token,
            "accessTokenExpirationTimestampMs": expiration
        }}}}}
    });
    Html(format!(
        "<!doctype html><script id=\"__NEXT_DATA__\" type=\"application/json\">{bootstrap}</script>"
    ))
}

pub(super) fn owned_entry(position: u64) -> Value {
    let recording = if position == 100 { 1 } else { position + 1 };
    json!({
        "itemV2": {
            "__typename": "PlaylistTrack",
            "data": {
                "__typename": "Track",
                "uri": format!("spotify:track:{recording:022}"),
                "name": format!("Owned recording {recording}"),
                "artists": {"items": [{
                    "uri": "spotify:artist:0000000000000000000001",
                    "profile": {"name": "Owned fixture artist"}
                }]},
                "duration": {"totalMilliseconds": 180_000},
                "discNumber": 1,
                "trackNumber": recording,
                "albumOfTrack": {
                    "uri": "spotify:album:0000000000000000000001",
                    "name": "Owned fixture album"
                }
            }
        }
    })
}
