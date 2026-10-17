use super::{
    Server,
    public_fixture::{Fixture, PLAYLIST_ID, owned_entry},
};
use crate::{Config, router};
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, Uri},
    response::{Html, Response},
    routing::get,
};
use hito_plugin_sdk::{ARTWORK_PATH, ArtworkEntity, ArtworkRequest, ProviderPlaylist};
use http_body_util::BodyExt;
use image::{ExtendedColorType, codecs::jpeg::JpegEncoder};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tower::ServiceExt;

const PROVIDER_ID: &str = "0000000000000000000002";

async fn artwork(app: &Router) -> Response {
    app.clone()
        .oneshot(
            Request::post(format!("/{ARTWORK_PATH}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ArtworkRequest {
                        entity: ArtworkEntity::Playlist,
                        provider_id: PROVIDER_ID.to_owned(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn playlist_mosaic_returns_the_largest_owned_raster_without_blocking_playlist_imports() {
    let witness = [
        format!("{:040x}", 1),
        format!("{:040x}", 2),
        format!("{:040x}", 3),
        format!("{:040x}", 4),
    ]
    .concat();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 85)
        .encode(
            &[200, 60, 30].repeat(640 * 640),
            640,
            640,
            ExtendedColorType::Rgb8,
        )
        .unwrap();
    let jpeg = Arc::new(jpeg);
    let served_jpeg = jpeg.clone();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let embed_calls = calls.clone();
    let image_calls = calls.clone();
    let image_base = Arc::new(OnceLock::<String>::new());
    let embedded_base = image_base.clone();
    let embedded_witness = witness.clone();
    let image_path = format!("/640/{witness}");
    let served_path = image_path.clone();
    let artwork_routes = Router::new()
        .route("/embed/playlist/0000000000000000000002", get(move |method: Method, headers: HeaderMap, uri: Uri| {
            let calls = embed_calls.clone();
            let image_base = embedded_base.clone();
            let witness = embedded_witness.clone();
            async move {
                assert_eq!(method, Method::GET);
                assert_eq!(uri.path_and_query().unwrap().as_str(), format!("/embed/playlist/{PROVIDER_ID}"));
                assert!(headers.get("authorization").is_none());
                assert!(headers.get("cookie").is_none());
                calls.lock().unwrap().push("embed");
                let images: Vec<_> = [640, 300, 60].into_iter().map(|size| json!({
                    "url": format!("{}{size}/{witness}", image_base.get().unwrap()),
                    "maxWidth": size,
                    "maxHeight": size
                })).collect();
                let bootstrap = json!({"props": {"pageProps": {"state": {"data": {"entity": {
                    "type": "playlist", "id": PROVIDER_ID,
                    "uri": format!("spotify:playlist:{PROVIDER_ID}"),
                    "visualIdentity": {"image": images}
                }}}}}});
                Html(format!("<!doctype html><script id=\"__NEXT_DATA__\" type=\"application/json\">{bootstrap}</script>"))
            }
        }))
        .route("/{size}/{witness}", get(move |method: Method, headers: HeaderMap, uri: Uri| {
            let calls = image_calls.clone();
            let jpeg = served_jpeg.clone();
            let expected_path = served_path.clone();
            async move {
                assert_eq!(method, Method::GET);
                assert_eq!(uri.path_and_query().unwrap().as_str(), expected_path);
                assert!(headers.get("authorization").is_none());
                assert!(headers.get("cookie").is_none());
                assert_eq!(headers["accept-encoding"], "identity");
                calls.lock().unwrap().push("image");
                ([("content-type", "image/jpeg")], jpeg.as_ref().clone())
            }
        }));
    let fixture = Fixture::with_routes(vec![owned_entry(0)], artwork_routes).await;
    image_base.set(fixture.server.url.to_string()).unwrap();
    let app = router(Config {
        public_base: fixture.server.url.clone(),
        query_endpoint: fixture.server.url.join("pathfinder/v2/query").unwrap(),
    })
    .unwrap();
    let plugin = Server::start(app.clone()).await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let response = client
        .post(plugin.url.join(ARTWORK_PATH).unwrap())
        .json(&ArtworkRequest {
            entity: ArtworkEntity::Playlist,
            provider_id: PROVIDER_ID.to_owned(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/jpeg");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response.headers()["etag"],
        format!("\"{:x}\"", Sha256::digest(jpeg.as_slice()))
    );
    assert_eq!(response.content_length(), Some(jpeg.len() as u64));
    assert_eq!(response.bytes().await.unwrap().as_ref(), jpeg.as_slice());
    assert_eq!(*calls.lock().unwrap(), ["embed", "image"]);

    let mut retained = Vec::new();
    for _ in 0..4 {
        let response = artwork(&app).await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body();
        let bytes = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(bytes.as_ref(), jpeg.as_slice());
        drop(body);
        retained.push(bytes);
    }
    assert_eq!(
        artwork(&app).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(*calls.lock().unwrap(), ["embed", "image"].repeat(5));
    let detail = client
        .post(plugin.url.join("v1/playlists/detail").unwrap())
        .json(&json!({"kind": "canonical", "playlist_id": PLAYLIST_ID}))
        .send()
        .await
        .unwrap();
    assert_eq!(detail.status(), StatusCode::OK);
    let playlist: ProviderPlaylist = detail.json().await.unwrap();
    playlist.validate().unwrap();
    assert_eq!(*fixture.calls.lock().unwrap(), ["embed", "query:0"]);
    assert_eq!(*calls.lock().unwrap(), ["embed", "image"].repeat(5));
    drop(retained);
}
