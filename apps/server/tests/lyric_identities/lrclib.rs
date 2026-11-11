use crate::fixture;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::{LyricContent, LyricsResponseV2};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;

const TOKEN: &str = "owned-composed-lrclib-service-credential";
const QUERY: &str = "Explicit owned local lyric search";
#[derive(Clone, Default)]
struct Upstream {
    calls: Arc<Mutex<Vec<&'static str>>>,
}
struct Server {
    url: String,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Server {
    async fn start(app: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (stop, receive) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = receive.await;
                })
                .await
                .unwrap();
        });
        Self {
            url,
            stop: Some(stop),
            task: Some(task),
        }
    }
    async fn finish(mut self) {
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(3), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
fn record() -> Value {
    json!({"id":4242,"trackName":"Owned LRCLIB fixture title","artistName":"Owned LRCLIB fixture artist","albumName":"Owned lyric metadata album","duration":123.45,"instrumental":false,"plainLyrics":"Owned composed plain lyric\nOwned second line","syncedLyrics":null})
}
async fn upstream(State(state): State<Upstream>, request: Request) -> Response {
    assert_eq!(request.method(), Method::GET);
    assert!(request.headers().get("authorization").is_none());
    assert!(request.headers().get("cookie").is_none());
    assert!(request.headers().get("x-hito-plugin-protocol").is_none());
    assert_eq!(request.headers()["accept"], "application/json");
    assert_eq!(request.headers()["accept-encoding"], "identity");
    assert!(
        request.headers()["user-agent"]
            .to_str()
            .unwrap()
            .starts_with("HitoLRCLIB/")
    );
    let path = request.uri().path().to_owned();
    let query = request.uri().query().map(str::to_owned);
    assert!(to_bytes(request.into_body(), 1).await.unwrap().is_empty());
    match path.as_str() {
        "/api/search" => {
            assert_eq!(
                url::form_urlencoded::parse(query.as_deref().unwrap().as_bytes())
                    .into_owned()
                    .collect::<Vec<_>>(),
                vec![("q".into(), QUERY.into())]
            );
            state.calls.lock().unwrap().push("search");
            Json(json!([record()])).into_response()
        }
        "/api/get/4242" => {
            assert!(query.is_none());
            state.calls.lock().unwrap().push("record");
            Json(record()).into_response()
        }
        _ => panic!("Composed LRCLIB reached an unexpected upstream endpoint"),
    }
}

#[tokio::test]
async fn a_scanned_local_recording_reviews_and_reads_plain_lyrics_through_the_actual_lrclib_router()
{
    let upstream_state = Upstream::default();
    let remote = Server::start(
        Router::new()
            .fallback(upstream)
            .with_state(upstream_state.clone()),
    )
    .await;
    let config = hito_plugin_lrclib::Config::new(TOKEN)
        .unwrap()
        .with_endpoint(remote.url.parse().unwrap())
        .unwrap();
    let shutdown = config.shutdown_handle();
    let plugin = Server::start(hito_plugin_lrclib::router(config).unwrap()).await;
    let library = fixture::Library::prepare().await;
    let installation = library
        .account
        .connect(&library.app, &plugin.url, "LRCLIB lyrics", Some(TOKEN))
        .await;
    assert_eq!(
        installation["manifest"]["capabilities"],
        json!(["search", "metadata", "lyrics"])
    );
    let before = library.snapshot().await;
    let base = format!("/api/v1/recordings/{}/lyrics", library.recording);
    let services = library
        .account
        .get(&library.app, &format!("{base}/services"))
        .await;
    assert_eq!(services["items"][0]["installation_id"], installation["id"]);
    assert!(
        upstream_state.calls.lock().unwrap().is_empty(),
        "Connecting and listing must not disclose local metadata upstream"
    );
    let (status,found)=library.account.request(&library.app,"POST",&format!("{base}/candidates"),json!({"installation_id":installation["id"],"installation_revision":installation["revision"],"query":QUERY})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["items"][0]["id"], "4242");
    let candidate = json!({"installation_id":installation["id"],"installation_revision":installation["revision"],"provider_recording_id":found["items"][0]["id"]});
    let (status, review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities/review"),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        review["candidate"]["album_title"],
        "Owned lyric metadata album"
    );
    assert!(review["candidate"]["recording"]["release"].is_null());
    let (status, identity) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities"),
            json!({"candidate":candidate,"review_stamp":review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(identity["selectable"], true);
    assert_eq!(
        identity["provider_recording"],
        json!({"id":"4242","revision":null})
    );
    assert_eq!(
        *upstream_state.calls.lock().unwrap(),
        vec!["search", "record", "record"]
    );
    assert_eq!(
        library
            .account
            .get(&library.app, &format!("{base}/identities"))
            .await["items"],
        json!([identity.clone()])
    );
    let (status,result)=library.account.request(&library.app,"POST",&format!("{base}/identities/read"),json!({"selector":identity["selector"],"installation_revision":installation["revision"]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["status"], "ready_v2");
    assert_eq!(result["selector"], identity["selector"]);
    assert_eq!(
        result["provider_recording"],
        json!({"id":"4242","revision":null})
    );
    let document: LyricsResponseV2 = serde_json::from_value(result["document"].clone()).unwrap();
    let lyric = document.lyrics.unwrap();
    assert!(
        matches!(lyric.content,LyricContent::Plain {ref lines} if lines==&vec!["Owned composed plain lyric".to_owned(),"Owned second line".to_owned()])
    );
    assert!(lyric.attribution.unwrap().creators.is_empty());
    assert_eq!(
        *upstream_state.calls.lock().unwrap(),
        vec!["search", "record", "record", "record"]
    );
    assert_eq!(
        library.snapshot().await,
        before,
        "The actual LRCLIB workflow must preserve local metadata, sources, library records and private collection state"
    );
    library.assert_original();
    shutdown.cancel();
    plugin.finish().await;
    remote.finish().await;
}
