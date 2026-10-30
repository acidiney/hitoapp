use crate::metadata_preferences_fixture as fixture;

use axum::http::StatusCode;
use fixture::Library;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};

const SETTINGS: &str = "/api/v1/metadata/settings";
const AUTOMATION: &str = "/api/v1/metadata/automation";

#[tokio::test]
async fn a_browser_account_saves_metadata_preferences_without_requesting_catalog_data() {
    let library = Library::prepare().await;
    let mut server = fixture::CatalogServer::start().await;
    let result = AssertUnwindSafe(exercise(&library, &server))
        .catch_unwind()
        .await;
    server.release_profile().await;
    let joined = server.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(joined, "The owned HTTP fixture must join shutdown");
}

async fn exercise(library: &Library, server: &fixture::CatalogServer) {
    let app = &library.app;
    let account = &library.accounts[0];
    let other = &library.accounts[1];
    let before = library.state().await;
    let default = json!({
        "revision": 0,
        "enabled": true,
        "providers": [{"kind": "builtin_musicbrainz"}],
        "automatic_artist_lookup": true,
        "automatic_artwork": true,
        "automatic_service_search": true,
    });
    let (status, initial) = account.request(app, "GET", SETTINGS, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        initial == default,
        "A new account must receive the enabled built-in MusicBrainz default"
    );
    assert!(other.get(app, SETTINGS).await == default);
    assert_eq!(
        server.counts().await,
        fixture::Counts {
            manifests: 0,
            descriptors: 0,
            profiles: 0,
            unexpected: 0
        }
    );

    let (status, service) = account
        .request(
            app,
            "POST",
            "/api/v1/plugins/services",
            json!({"name": "Owned preferences catalog", "base_url": server.url}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = account
        .request(
            app,
            "POST",
            "/api/v1/plugins",
            json!({
                "service_id": service["id"],
                "service_revision": service["revision"],
                "bearer_token": null,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(installation["manifest"]["capabilities"] == json!(["catalog"]));
    let plugins = account.get(app, "/api/v1/plugins").await;
    let selected = json!({"kind": "http_plugin", "installation_id": installation["id"]});
    let (status, saved) = account
        .request(
            app,
            "PUT",
            SETTINGS,
            json!({"revision": initial["revision"], "enabled": true, "providers": [selected]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        saved
            == json!({"revision": 1, "enabled": true, "providers": [selected],
                "automatic_artist_lookup": true, "automatic_artwork": true,
                "automatic_service_search": true}),
        "Saving preferences must return the exact selected personal provider and next revision"
    );
    assert!(
        account.get(app, SETTINGS).await == saved,
        "The authenticated read must return the exact stored metadata preferences"
    );
    assert!(
        other.get(app, SETTINGS).await == default,
        "Saving one account's metadata preferences must preserve another account's default"
    );
    assert!(
        account.get(app, "/api/v1/plugins").await == plugins,
        "Selecting a provider must preserve its active personal installation and service approval"
    );
    assert!(
        library.state().await == before,
        "Metadata preferences must preserve both accounts' library, provenance and private collection state"
    );
    assert_eq!(
        server.counts().await,
        fixture::Counts {
            manifests: 1,
            descriptors: 0,
            profiles: 0,
            unexpected: 0
        }
    );
}

#[tokio::test]
async fn automatic_lookup_and_artwork_default_on_and_save_per_account() {
    let library = Library::prepare().await;
    let app = &library.app;
    let (account, other) = (&library.accounts[0], &library.accounts[1]);
    let initial = account.get(app, SETTINGS).await;
    assert_eq!(initial["automatic_artist_lookup"], true);
    assert_eq!(initial["automatic_artwork"], true);
    assert_eq!(initial["automatic_service_search"], true);

    let (status, saved) = account
        .request(
            app,
            "PUT",
            AUTOMATION,
            json!({"automatic_artist_lookup": false}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        saved["revision"], 0,
        "Automation choices are not provider revisions"
    );
    assert_eq!(saved["automatic_artist_lookup"], false);
    assert_eq!(
        saved["automatic_artwork"], true,
        "An omitted choice is kept"
    );
    assert!(account.get(app, SETTINGS).await == saved);
    let theirs = other.get(app, SETTINGS).await;
    assert_eq!(theirs["automatic_artist_lookup"], true);

    let (status, saved) = account
        .request(app, "PUT", AUTOMATION, json!({"automatic_artwork": false}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        saved["automatic_artist_lookup"], false,
        "An omitted choice is kept"
    );
    assert_eq!(saved["automatic_artwork"], false);
    let (status, saved) = account
        .request(
            app,
            "PUT",
            AUTOMATION,
            json!({"automatic_service_search": false}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["automatic_service_search"], false);
    assert_eq!(saved["automatic_artwork"], false, "Other choices are kept");
    assert_eq!(
        other.get(app, SETTINGS).await["automatic_service_search"],
        true
    );
    let (status, _) = account
        .request(app, "PUT", AUTOMATION, json!({"automatic_artwork": "yes"}))
        .await;
    assert!(status.is_client_error());
    let (status, _) = account
        .request(app, "PUT", AUTOMATION, json!({"unknown": true}))
        .await;
    assert!(status.is_client_error());
    let saved_providers = account
        .request(
            app,
            "PUT",
            SETTINGS,
            json!({"revision": 0, "enabled": true, "providers": [{"kind": "builtin_musicbrainz"}]}),
        )
        .await
        .1;
    assert_eq!(
        saved_providers["automatic_artwork"], false,
        "Saving providers keeps the automation choices"
    );
}

#[tokio::test]
async fn the_artist_matching_status_is_per_account_and_reports_the_setting() {
    let library = Library::prepare().await;
    let app = &library.app;
    let (account, other) = (&library.accounts[0], &library.accounts[1]);
    let status = account.get(app, "/api/v1/artist-matching/status").await;
    assert_eq!(status["enabled"], true);
    assert!(["idle", "queued"].contains(&status["state"].as_str().unwrap()));
    assert_eq!(status["last_finished_at"], Value::Null);
    account
        .request(
            app,
            "PUT",
            AUTOMATION,
            json!({"automatic_artist_lookup": false}),
        )
        .await;
    assert_eq!(
        account.get(app, "/api/v1/artist-matching/status").await["enabled"],
        false
    );
    assert_eq!(
        other.get(app, "/api/v1/artist-matching/status").await["enabled"],
        true
    );
    let (code, _) = account
        .request(app, "GET", "/api/v1/artist-matching/status", Value::Null)
        .await;
    assert_eq!(code, StatusCode::OK);
}
