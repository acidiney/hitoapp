use super::Server;
use crate::{Config, router};
use axum::{
    Router,
    http::{Method, StatusCode, Uri},
    response::Html,
    routing::get,
};
use hito_plugin_sdk::{ARTWORK_PATH, ArtworkEntity, ArtworkRequest};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

const PROVIDER_ID: &str = "0000000000000000000002";
const IMAGE: &[u8] = include_bytes!("fixtures/artwork.png");

struct Fixture {
    upstream: Server,
    plugin: Server,
    client: reqwest::Client,
    location: Arc<Mutex<String>>,
    calls: Arc<Mutex<Vec<&'static str>>>,
    witness: String,
}

impl Fixture {
    async fn start() -> Self {
        let location = Arc::new(Mutex::new(String::new()));
        let embedded_location = location.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let embed_calls = calls.clone();
        let image_calls = calls.clone();
        let witness = "0123456789abcdef0123456789abcdef01234567".repeat(4);
        let image_path = format!("/640/{witness}");
        let upstream = Server::start(
        Router::new()
            .route(
                "/embed/playlist/0000000000000000000002",
                get(move |method: Method, uri: Uri| {
                    let calls = embed_calls.clone();
                    let location = embedded_location.clone();
                    async move {
                        assert_eq!(method, Method::GET);
                        assert_eq!(uri.path_and_query().unwrap().as_str(), format!("/embed/playlist/{PROVIDER_ID}"));
                        calls.lock().unwrap().push("embed");
                        let bootstrap = json!({"props": {"pageProps": {"state": {"data": {"entity": {
                            "type": "playlist", "id": PROVIDER_ID,
                            "uri": format!("spotify:playlist:{PROVIDER_ID}"),
                            "visualIdentity": {"image": [{
                                "url": location.lock().unwrap().clone(),
                                "maxWidth": 640, "maxHeight": 640
                            }]}
                        }}}}}});
                        Html(format!("<!doctype html><script id=\"__NEXT_DATA__\" type=\"application/json\">{bootstrap}</script>"))
                    }
                }),
            )
            .route("/640/{witness}", get(move |method: Method, uri: Uri| {
                let calls = image_calls.clone();
                let path = image_path.clone();
                async move {
                    assert_eq!(method, Method::GET);
                    assert_eq!(uri.path_and_query().unwrap().as_str(), path);
                    calls.lock().unwrap().push("image");
                    ([("content-type", "image/png")], IMAGE)
                }
            })),
    ).await;
        let plugin = Server::start(
            router(Config {
                public_base: upstream.url.clone(),
                query_endpoint: upstream.url.join("pathfinder/v2/query").unwrap(),
            })
            .unwrap(),
        )
        .await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        Self {
            upstream,
            plugin,
            client,
            location,
            calls,
            witness,
        }
    }

    async fn request(&self, location: String) -> reqwest::Response {
        *self.location.lock().unwrap() = location;
        self.client
            .post(self.plugin.url.join(ARTWORK_PATH).unwrap())
            .json(&ArtworkRequest {
                entity: ArtworkEntity::Playlist,
                provider_id: PROVIDER_ID.to_owned(),
            })
            .send()
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn mosaic_dot_segments_are_rejected_before_the_normalized_image_url_is_requested() {
    let fixture = Fixture::start().await;
    let valid = fixture
        .request(format!("{}640/{}", fixture.upstream.url, fixture.witness))
        .await;
    assert_eq!(valid.status(), StatusCode::OK);
    assert_eq!(valid.bytes().await.unwrap().as_ref(), IMAGE);
    assert_eq!(*fixture.calls.lock().unwrap(), ["embed", "image"]);

    let invalid = fixture
        .request(format!(
            "{}640/../640/{}",
            fixture.upstream.url, fixture.witness
        ))
        .await;
    assert_eq!(invalid.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        invalid.json::<serde_json::Value>().await.unwrap(),
        json!({"error": "Spotify returned invalid artwork"})
    );
    assert_eq!(*fixture.calls.lock().unwrap(), ["embed", "image", "embed"]);
}

#[tokio::test]
async fn malformed_mosaic_locations_are_rejected_without_requesting_image_bytes() {
    let fixture = Fixture::start().await;
    let valid = fixture
        .request(format!("{}640/{}", fixture.upstream.url, fixture.witness))
        .await;
    assert_eq!(valid.status(), StatusCode::OK);
    assert_eq!(valid.bytes().await.unwrap().as_ref(), IMAGE);
    assert_eq!(*fixture.calls.lock().unwrap(), ["embed", "image"]);

    let other_calls = Arc::new(Mutex::new(0));
    let captured = other_calls.clone();
    let other_origin = Server::start(Router::new().fallback(move || {
        let calls = captured.clone();
        async move {
            *calls.lock().unwrap() += 1;
            ([("content-type", "image/png")], IMAGE)
        }
    }))
    .await;
    let base = fixture.upstream.url.as_str();
    let witness = &fixture.witness;
    let mut credentials = fixture
        .upstream
        .url
        .join(&format!("640/{witness}"))
        .unwrap();
    credentials.set_username("owned").unwrap();
    credentials.set_password(Some("private")).unwrap();
    for (label, location) in [
        ("zero size", format!("{base}0/{witness}")),
        ("oversized size", format!("{base}1025/{witness}")),
        ("noncanonical size", format!("{base}0640/{witness}")),
        (
            "short mosaic witness",
            format!("{base}640/{}", &witness[..159]),
        ),
        ("long mosaic witness", format!("{base}640/{witness}0")),
        (
            "nonhex mosaic witness",
            format!("{base}640/{}g", &witness[..159]),
        ),
        ("extra segment", format!("{base}640/{witness}/extra")),
        ("query", format!("{base}640/{witness}?owned=1")),
        ("fragment", format!("{base}640/{witness}#owned")),
        ("userinfo", credentials.to_string()),
        (
            "alternate authority",
            format!("{}640/{witness}", other_origin.url),
        ),
    ] {
        fixture.calls.lock().unwrap().clear();
        let response = fixture.request(location).await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "case: {label}");
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap(),
            json!({"error": "Spotify returned invalid artwork"}),
            "case: {label}"
        );
        assert_eq!(*fixture.calls.lock().unwrap(), ["embed"], "case: {label}");
        assert_eq!(*other_calls.lock().unwrap(), 0, "case: {label}");
    }
}
