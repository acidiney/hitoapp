#[allow(dead_code)]
#[path = "fixtures/artist_review_account.rs"]
mod account;
#[allow(dead_code)]
#[path = "fixtures/lyric_identity_library.rs"]
mod fixture;
#[path = "fixtures/automatic_lyric_lrclib.rs"]
mod lrclib;
#[allow(dead_code)]
#[path = "fixtures/automatic_lyric_provider.rs"]
mod provider;
#[allow(dead_code)]
#[path = "api/support.rs"]
mod support;

#[path = "automatic_lyrics/authority.rs"]
mod authority;
#[path = "automatic_lyrics/availability.rs"]
mod availability;
#[path = "automatic_lyrics/cancellation.rs"]
mod cancellation;
#[path = "automatic_lyrics/isolation.rs"]
mod isolation;
#[path = "automatic_lyrics/logging.rs"]
mod logging;
#[path = "automatic_lyrics/matching.rs"]
mod matching;
#[path = "automatic_lyrics/protocol.rs"]
mod protocol;
#[path = "automatic_lyrics/schema.rs"]
mod schema;
#[allow(dead_code)]
#[path = "automatic_lyrics/setup.rs"]
mod setup;
#[allow(dead_code)]
#[path = "fixtures/manual_source.rs"]
mod source_provider;
#[path = "automatic_lyrics/timed_priority.rs"]
mod timed_priority;
#[path = "automatic_lyrics/timed_priority_lifecycle.rs"]
mod timed_priority_lifecycle;
#[path = "automatic_lyrics/timed_priority_manual.rs"]
mod timed_priority_manual;
#[path = "automatic_lyrics/timed_priority_matching.rs"]
mod timed_priority_matching;
#[path = "automatic_lyrics/timed_priority_protocol.rs"]
mod timed_priority_protocol;

use axum::http::StatusCode;
use serde_json::json;
use setup::commit_metadata;

#[tokio::test]
async fn automatic_lyrics_uses_committed_local_metadata_and_the_actual_lrclib_router_without_retaining_an_identity()
 {
    let remote = lrclib::Upstream::start().await;
    let config = hito_plugin_lrclib::Config::new(lrclib::TOKEN)
        .unwrap()
        .with_endpoint(remote.server.url.parse().unwrap())
        .unwrap();
    let shutdown = config.shutdown_handle();
    let plugin = lrclib::Server::start(hito_plugin_lrclib::router(config).unwrap()).await;
    let library = fixture::Library::prepare().await;
    commit_metadata(&library, lrclib::TITLE, lrclib::ARTIST).await;
    let installation = library
        .account
        .connect(
            &library.app,
            &plugin.url,
            "Owned automatic LRCLIB",
            Some(lrclib::TOKEN),
        )
        .await;
    let base = format!("/api/v1/recordings/{}/lyrics", library.recording);
    let before = library.snapshot().await;
    assert_eq!(
        library
            .account
            .get(&library.app, &format!("{base}/identities"))
            .await["items"],
        json!([])
    );
    remote.assert_calls(&[]);
    let (status, response) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/automatic"),
            json!({}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Automatic read must work without a saved provider identity"
    );
    assert_eq!(response["status"], "ready_v2");
    assert_eq!(response["recording_id"], library.recording);
    assert_eq!(response["installation_id"], installation["id"]);
    assert_eq!(response["installation_revision"], installation["revision"]);
    assert_eq!(response["label"], "LRCLIB lyrics");
    assert_eq!(
        response["provider_recording"],
        json!({"id":"4242","revision":null})
    );
    assert_eq!(
        response["document"]["lyrics"]["content"],
        json!({"kind":"plain","lines":["Owned automatic plain text"]})
    );
    assert_eq!(response["document_digest"].as_str().unwrap().len(), 64);
    assert!(response.get("selector").is_none());
    remote.assert_calls(&["search", "record", "record"]);
    assert_eq!(
        library
            .account
            .get(&library.app, &format!("{base}/identities"))
            .await["items"],
        json!([])
    );
    assert_eq!(library.snapshot().await, before);
    library.assert_original();
    shutdown.cancel();
    plugin.finish().await;
    remote.server.finish().await;
}

#[tokio::test]
async fn automatic_lyrics_rejects_a_live_version_identified_only_by_refetched_album_metadata() {
    let provider = provider::Provider::start().await;
    provider.reply("metadata", json!({"recording":provider::recording(),"album_title":"Owned album Live at the owned venue"}));
    let library = fixture::Library::prepare().await;
    commit_metadata(&library, lrclib::TITLE, lrclib::ARTIST).await;
    library
        .account
        .connect(
            &library.app,
            provider.url(),
            "Owned automatic service",
            None,
        )
        .await;
    let before = library.snapshot().await;
    let (status, response) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("/api/v1/recordings/{}/lyrics/automatic", library.recording),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response["status"], "not_found",
        "Album-only live versions must not be guessed as the committed studio recording"
    );
    assert_eq!(provider.calls(), vec!["manifest", "search", "metadata"]);
    assert_eq!(library.snapshot().await, before);
    library.assert_original();
    provider.finish().await;
}
