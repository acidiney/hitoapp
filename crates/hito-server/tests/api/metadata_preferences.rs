use crate::metadata_preferences_fixture as fixture;

use axum::http::StatusCode;
use fixture::Library;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};

const SETTINGS: &str = "/api/v1/metadata/settings";

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
        saved == json!({"revision": 1, "enabled": true, "providers": [selected]}),
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
