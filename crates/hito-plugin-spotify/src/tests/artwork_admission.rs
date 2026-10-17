use super::public_fixture::{Fixture, PLAYLIST_ID, owned_entry};
use crate::{Config, router};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::{Html, Response},
    routing::get,
};
use hito_plugin_sdk::{PlaylistPage, ProviderPlaylist};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tower::ServiceExt;

const PROVIDER_ID: &str = "0000000000000000000002";
const IMAGE: &[u8] = include_bytes!("fixtures/artwork.png");

async fn post(app: &Router, path: &str, payload: Value) -> Response {
    app.clone()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn four_retained_artwork_responses_preserve_playlist_detail_and_cached_items_admission() {
    let image_url = Arc::new(OnceLock::<String>::new());
    let embedded_image_url = image_url.clone();
    let artwork_routes = Router::new()
        .route(
            "/embed/track/0000000000000000000002",
            get(move || {
                let image_url = embedded_image_url.clone();
                async move {
                    let bootstrap = json!({
                        "props": {"pageProps": {"state": {"data": {"entity": {
                            "type": "track",
                            "id": PROVIDER_ID,
                            "uri": format!("spotify:track:{PROVIDER_ID}"),
                            "visualIdentity": {"image": [{
                                "url": image_url.get().unwrap(),
                                "maxWidth": 2,
                                "maxHeight": 2
                            }]}
                        }}}}}
                    });
                    Html(format!(
                        "<!doctype html><script id=\"__NEXT_DATA__\" type=\"application/json\">{bootstrap}</script>"
                    ))
                }
            }),
        )
        .route("/image/{asset}", get(|| async { ([("content-type", "image/png")], IMAGE) }));
    let fixture = Fixture::with_routes(vec![owned_entry(0)], artwork_routes).await;
    image_url
        .set(
            fixture
                .server
                .url
                .join(&format!("image/{:040}", 2))
                .unwrap()
                .to_string(),
        )
        .unwrap();
    let app = router(Config {
        public_base: fixture.server.url.clone(),
        query_endpoint: fixture.server.url.join("pathfinder/v2/query").unwrap(),
    })
    .unwrap();
    let detail_request = json!({"kind": "canonical", "playlist_id": PLAYLIST_ID});
    let detail = post(&app, "/v1/playlists/detail", detail_request.clone()).await;
    assert_eq!(detail.status(), StatusCode::OK);
    let playlist: ProviderPlaylist =
        serde_json::from_slice(&detail.into_body().collect().await.unwrap().to_bytes()).unwrap();
    playlist.validate().unwrap();
    let items_request = json!({
        "playlist_id": playlist.id,
        "snapshot": playlist.snapshot,
        "cursor": null,
        "limit": 100
    });
    let items = post(&app, "/v1/playlists/items", items_request.clone()).await;
    assert_eq!(items.status(), StatusCode::OK);
    let primed: PlaylistPage =
        serde_json::from_slice(&items.into_body().collect().await.unwrap().to_bytes()).unwrap();
    primed.validate(100).unwrap();
    assert_eq!(primed.items.len(), 1);

    let mut retained = Vec::new();
    for _ in 0..4 {
        let response = post(
            &app,
            "/v1/artwork",
            json!({
                "entity": "recording", "provider_id": PROVIDER_ID
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body();
        let bytes = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(bytes.as_ref(), IMAGE);
        drop(body);
        retained.push(bytes);
    }

    let (detail, items) = tokio::join!(
        tokio::time::timeout(
            Duration::from_secs(1),
            post(&app, "/v1/playlists/detail", detail_request)
        ),
        tokio::time::timeout(
            Duration::from_secs(1),
            post(&app, "/v1/playlists/items", items_request)
        ),
    );
    drop(retained);
    assert_eq!(
        (
            detail.as_ref().ok().map(Response::status),
            items.as_ref().ok().map(Response::status)
        ),
        (Some(StatusCode::OK), Some(StatusCode::OK)),
    );
    let refreshed: ProviderPlaylist = serde_json::from_slice(
        &detail
            .unwrap()
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes(),
    )
    .unwrap();
    refreshed.validate().unwrap();
    let page: PlaylistPage = serde_json::from_slice(
        &items
            .unwrap()
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes(),
    )
    .unwrap();
    page.validate(100).unwrap();
    assert_eq!(refreshed.snapshot, playlist.snapshot);
    assert_eq!(page.snapshot, primed.snapshot);
    assert!(page.items == primed.items);
    assert_eq!(
        *fixture.calls.lock().unwrap(),
        ["embed", "query:0", "embed", "query:0"]
    );
}
