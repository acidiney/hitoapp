use crate::{
    account::exchange,
    credit_member_review_credentials::private_snapshot,
    credit_member_review_preview_fixture::{Context, Library, private},
};
use axum::http::{HeaderMap, Method, StatusCode};
use hito_core::test_support::artist_catalog_discovery_service as catalog;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::task::JoinHandle;

pub(crate) type Response = (StatusCode, HeaderMap, Value);

struct Pending {
    gate: Arc<catalog::ProfileGate>,
    task: JoinHandle<Response>,
    entered: bool,
}

pub(crate) struct Owned {
    pub(crate) library: Library,
    pending: Option<Pending>,
}

impl Owned {
    pub(crate) fn new() -> Self {
        Self {
            library: Library::new(),
            pending: None,
        }
    }

    pub(crate) fn provider(&self) -> &catalog::DiscoveryServer {
        self.library.provider.as_ref().unwrap()
    }

    pub(crate) fn hold(&mut self, context: &Context) {
        assert!(self.pending.is_none());
        let gate = self.provider().hold_profile();
        let app = context.app.clone();
        let path = path(context);
        let body = body(context);
        let headers = context.account.headers();
        let task = tokio::spawn(async move { exchange(&app, "POST", &path, body, headers).await });
        self.pending = Some(Pending {
            gate,
            task,
            entered: false,
        });
    }

    pub(crate) async fn entered(&mut self) -> bool {
        let pending = self.pending.as_mut().unwrap();
        pending.entered = pending.gate.entered_within(Duration::from_secs(2)).await;
        pending.entered
    }

    pub(crate) fn pending(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| !pending.task.is_finished())
    }

    pub(crate) async fn complete(&mut self) -> Response {
        self.provider().release_profile();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            &mut self.pending.as_mut().unwrap().task,
        )
        .await;
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                panic!("The held member preview must join within its existing request deadline")
            }
        };
        let pending = self.pending.take().unwrap();
        assert!(
            !pending.entered || pending.gate.has_retired(),
            "The held profile body must retire"
        );
        result.expect("The owned member preview must join")
    }

    pub(crate) async fn close(&mut self) {
        let (joined, held) = if let Some(mut pending) = self.pending.take() {
            self.provider().release_profile();
            pending.gate.release();
            pending.task.abort();
            let joined = match tokio::time::timeout(Duration::from_secs(5), &mut pending.task).await
            {
                Ok(Ok(_)) => true,
                Ok(Err(error)) => error.is_cancelled(),
                Err(_) => false,
            };
            (joined, Some((pending.gate, pending.entered)))
        } else {
            (true, None)
        };
        self.library.retire().await;
        assert!(joined, "The owned preview request task must retire");
        assert!(
            held.is_none_or(|(gate, entered)| !entered || gate.has_retired()),
            "The owned held profile body must retire"
        );
    }
}

pub(crate) fn path(context: &Context) -> String {
    format!("{}/preview", context.member_path)
}

pub(crate) fn body(context: &Context) -> Value {
    json!({"context": context.context, "provider_artist_id": catalog::SECOND_ARTIST})
}

pub(crate) async fn state(context: &Context, playlist: &str) -> Value {
    let recording = context.context["recording_id"].as_str().unwrap();
    let mut snapshot = private_snapshot(&context.app, &context.account, recording, playlist).await;
    snapshot["activity"] = context.account.get(&context.app, "/api/v1/activity").await;
    snapshot
}

pub(crate) async fn diagnose(owned: &Owned, context: &Context, baseline: &Value) -> Value {
    owned.provider().reject_profile();
    let rejected = exchange(
        &context.app,
        "POST",
        &path(context),
        body(context),
        context.account.headers(),
    )
    .await;
    private(&rejected.1);
    assert_eq!(rejected.0, StatusCode::CONFLICT);
    assert!(rejected.2["code"] == "plugin_credentials_required");
    let installation = &context.context["provider"]["installation_id"];
    assert!(
        rejected.2["plugin_connection"]
            == json!({
                "installation_id": installation,
                "installation_revision": context.context["connection"]["installation_revision"],
            })
    );
    assert_eq!(rejected.2.as_object().unwrap().len(), 3);
    assert!(!rejected.2.to_string().contains(catalog::TOKEN));
    let mut required = baseline.clone();
    let row = required["installations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == *installation)
        .unwrap();
    assert!(row["revision"] == context.context["connection"]["installation_revision"]);
    row["health"] = json!({"state": "credentials_required", "reason": null, "since": null});
    assert!(
        context.account.get(&context.app, "/api/v1/plugins").await == required,
        "Only the current owned catalog connection must gain the credential diagnosis"
    );
    owned.provider().release_profile();
    required
}

pub(crate) fn protocol(owned: &Owned, before: usize) {
    let requests = owned.provider().requests();
    assert_eq!(requests.len(), before + 4);
    let expected = [
        (Method::GET, "/prefix/v1/catalog/capabilities", Value::Null),
        (
            Method::POST,
            "/prefix/v1/catalog/artists/profile",
            json!({"artist_id": catalog::SECOND_ARTIST}),
        ),
        (Method::GET, "/prefix/v1/catalog/capabilities", Value::Null),
        (
            Method::POST,
            "/prefix/v1/catalog/artists/profile",
            json!({"artist_id": catalog::SECOND_ARTIST}),
        ),
    ];
    for (request, (method, path, body)) in requests[before..].iter().zip(expected) {
        assert!(
            request.method == method && request.path == path && request.body == body,
            "The provider must receive only the exact ordinary profile request"
        );
        assert!(request.headers["x-hito-plugin-protocol"] == "1");
        assert!(
            request.headers["authorization"] == format!("Bearer {}", catalog::TOKEN).as_str(),
            "The owned provider credential must remain scoped to its request"
        );
    }
    assert_eq!(owned.provider().unexpected(), 0);
}
