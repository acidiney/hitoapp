use crate::credit_member_boundary_fixture::{
    Library, Prepared, accepted, catalog, origin, preserved_context, private, snapshot,
};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use futures_util::FutureExt;
use hito_core::{
    CreditMemberReviewContext, MetadataProvider, test_support::hang_guard::hang_guard,
};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn member_context_respects_main_role_access_and_committed_provider_authority() {
    let mut library = Library::new();
    let outcome = AssertUnwindSafe(Box::pin(async {
        let (app, prepared) = hang_guard(
            "member context boundary HTTP library and reviewed whole choice",
            Box::pin(library.prepare()),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(exercise(
                &app,
                &prepared,
                library.origin.as_ref().unwrap(),
                library.provider.as_ref().unwrap(),
            )),
        )
        .await
        .expect("The member context boundary HTTP workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    library.retire().await;
    let checks = match outcome {
        Ok(response) => response,
        Err(panic) => resume_unwind(panic),
    };
    for check in checks {
        assert_eq!(check.status, check.expected_status, "{}", check.name);
        private(&check.headers);
        match check.expected_context {
            Some(expected) => assert_eq!(check.body, expected, "{}", check.name),
            None => {
                assert!(
                    serde_json::from_value::<CreditMemberReviewContext>(check.body.clone())
                        .is_err(),
                    "{}",
                    check.name
                );
                assert!(check.body["error"].is_string(), "{}", check.name);
            }
        }
    }
}

struct ContextCheck {
    name: &'static str,
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
    expected_status: StatusCode,
    expected_context: Option<Value>,
}

async fn exercise(
    app: &Router,
    prepared: &Prepared,
    origin: &origin::ProviderServer,
    provider: &catalog::CatalogServer,
) -> Vec<ContextCheck> {
    let mut checks = Vec::new();
    let before = snapshot(app, &prepared.owner, &prepared.owner_selection).await;
    assert_eq!(before["choice"]["revision"], 1);
    assert_eq!(before["recordings"]["items"][0]["liked"], true);
    assert_eq!(
        before["metadata"]["title"]["personal"],
        "Private owned trio title"
    );
    assert_eq!(before["progress"][0]["position_ms"], 400);
    assert_eq!(before["sources"], json!([]));
    assert_eq!(before["groups"]["total"], 1);
    let peer_before = snapshot(app, &prepared.peer, &prepared.peer_selection).await;
    let request = |recording: &str, artist: &str| {
        format!("/api/v1/recordings/{recording}/credit-members/{artist}/artist-review/context")
    };
    let path = request(&prepared.owner_selection.recording, &prepared.main);
    let mut expected = prepared.expected.clone();
    let initial = preserved_context(
        app,
        prepared,
        origin,
        provider,
        &prepared.owner,
        &path,
        &prepared.provider,
    )
    .await;
    checks.push(ContextCheck {
        name: "current main",
        status: initial.0,
        headers: initial.1,
        body: initial.2,
        expected_status: StatusCode::OK,
        expected_context: Some(expected.clone()),
    });
    for (name, account, recording, artist) in [
        (
            "absent artist",
            &prepared.owner,
            prepared.owner_selection.recording.as_str(),
            Uuid::nil().to_string(),
        ),
        (
            "foreign artist",
            &prepared.owner,
            prepared.owner_selection.recording.as_str(),
            prepared.foreign.clone(),
        ),
        (
            "unreadable recording",
            &prepared.owner,
            prepared.peer_selection.recording.as_str(),
            prepared.main.clone(),
        ),
        (
            "peer recording denial",
            &prepared.peer,
            prepared.owner_selection.recording.as_str(),
            prepared.main.clone(),
        ),
    ] {
        let response = preserved_context(
            app,
            prepared,
            origin,
            provider,
            account,
            &request(recording, &artist),
            &prepared.provider,
        )
        .await;
        checks.push(ContextCheck {
            name,
            status: response.0,
            headers: response.1,
            body: response.2,
            expected_status: StatusCode::NOT_FOUND,
            expected_context: None,
        });
    }
    let settings = prepared.owner.get(app, "/api/v1/metadata/settings").await;
    let disabled = accepted(
        app,
        &prepared.owner,
        "PUT",
        "/api/v1/metadata/settings",
        json!({
            "revision": settings["revision"], "enabled": false, "providers": [prepared.provider],
        }),
    )
    .await;
    let response = preserved_context(
        app,
        prepared,
        origin,
        provider,
        &prepared.owner,
        &path,
        &prepared.provider,
    )
    .await;
    checks.push(ContextCheck {
        name: "disabled selected policy",
        status: response.0,
        headers: response.1,
        body: response.2,
        expected_status: StatusCode::CONFLICT,
        expected_context: None,
    });
    let restored = accepted(
        app,
        &prepared.owner,
        "PUT",
        "/api/v1/metadata/settings",
        json!({
            "revision": disabled["revision"], "enabled": true, "providers": [prepared.provider],
        }),
    )
    .await;
    expected["policy_revision"] = restored["revision"].clone();
    let response = preserved_context(
        app,
        prepared,
        origin,
        provider,
        &prepared.owner,
        &path,
        &prepared.provider,
    )
    .await;
    checks.push(ContextCheck {
        name: "current restored policy",
        status: response.0,
        headers: response.1,
        body: response.2,
        expected_status: StatusCode::OK,
        expected_context: Some(expected.clone()),
    });
    let installation_id = prepared.installation["id"].as_str().unwrap();
    let refreshed = accepted(
        app,
        &prepared.owner,
        "POST",
        &format!("/api/v1/plugins/{installation_id}/refresh"),
        json!({"revision": prepared.installation["revision"]}),
    )
    .await;
    assert_eq!(refreshed["kind"], "updated");
    assert_eq!(
        refreshed["installation"]["revision"].as_i64().unwrap(),
        prepared.installation["revision"].as_i64().unwrap() + 1
    );
    expected["connection"]["installation_revision"] = refreshed["installation"]["revision"].clone();
    let response = preserved_context(
        app,
        prepared,
        origin,
        provider,
        &prepared.owner,
        &path,
        &prepared.provider,
    )
    .await;
    checks.push(ContextCheck {
        name: "current refreshed installation",
        status: response.0,
        headers: response.1,
        body: response.2,
        expected_status: StatusCode::OK,
        expected_context: Some(expected.clone()),
    });
    let (status, _) = prepared
        .owner
        .request(
            app,
            "DELETE",
            &format!("/api/v1/plugins/{installation_id}"),
            json!({"revision": refreshed["installation"]["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let response = preserved_context(
        app,
        prepared,
        origin,
        provider,
        &prepared.owner,
        &path,
        &prepared.provider,
    )
    .await;
    checks.push(ContextCheck {
        name: "removed installation",
        status: response.0,
        headers: response.1,
        body: response.2,
        expected_status: StatusCode::NOT_FOUND,
        expected_context: None,
    });
    let (status, replacement) = prepared.owner.request(app, "POST", "/api/v1/plugins", json!({
        "service_id": prepared.installation["service_id"], "service_revision": prepared.service_revision, "bearer_token": null,
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(replacement["id"], prepared.installation["id"]);
    let replaced_provider = MetadataProvider::HttpPlugin {
        installation_id: replacement["id"].as_str().unwrap().into(),
    };
    let response = preserved_context(
        app,
        prepared,
        origin,
        provider,
        &prepared.owner,
        &path,
        &prepared.provider,
    )
    .await;
    checks.push(ContextCheck {
        name: "obsolete installation after replacement",
        status: response.0,
        headers: response.1,
        body: response.2,
        expected_status: StatusCode::NOT_FOUND,
        expected_context: None,
    });
    let response = preserved_context(
        app,
        prepared,
        origin,
        provider,
        &prepared.owner,
        &path,
        &replaced_provider,
    )
    .await;
    checks.push(ContextCheck {
        name: "unselected replacement",
        status: response.0,
        headers: response.1,
        body: response.2,
        expected_status: StatusCode::CONFLICT,
        expected_context: None,
    });
    let selected = accepted(
        app,
        &prepared.owner,
        "PUT",
        "/api/v1/metadata/settings",
        json!({
            "revision": restored["revision"], "enabled": true, "providers": [replaced_provider],
        }),
    )
    .await;
    expected["policy_revision"] = selected["revision"].clone();
    expected["provider"] = json!(replaced_provider);
    expected["connection"]["installation_revision"] = replacement["revision"].clone();
    let response = preserved_context(
        app,
        prepared,
        origin,
        provider,
        &prepared.owner,
        &path,
        &replaced_provider,
    )
    .await;
    checks.push(ContextCheck {
        name: "selected current replacement",
        status: response.0,
        headers: response.1,
        body: response.2,
        expected_status: StatusCode::OK,
        expected_context: Some(expected),
    });
    let (status, _) = prepared
        .owner
        .request(
            app,
            "DELETE",
            &format!(
                "/api/v1/plugins/services/{}",
                prepared.installation["service_id"].as_str().unwrap()
            ),
            json!({"revision": prepared.service_revision}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let response = preserved_context(
        app,
        prepared,
        origin,
        provider,
        &prepared.owner,
        &path,
        &replaced_provider,
    )
    .await;
    checks.push(ContextCheck {
        name: "withdrawn service",
        status: response.0,
        headers: response.1,
        body: response.2,
        expected_status: StatusCode::NOT_FOUND,
        expected_context: None,
    });
    assert_eq!(
        snapshot(app, &prepared.owner, &prepared.owner_selection).await,
        before
    );
    assert_eq!(
        snapshot(app, &prepared.peer, &prepared.peer_selection).await,
        peer_before
    );
    checks
}
