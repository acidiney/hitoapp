use crate::{account, source_credential_fixture as fixture};
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use hito_plugin_sdk::{
    Capability, HEALTH_PATH, HealthReport, MANIFEST_PATH, Manifest, PROTOCOL_VERSION, SEARCH_PATH,
    SearchRequest,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use tokio::{sync::Notify, task::JoinSet};
use tracing::{Subscriber, instrument::WithSubscriber};
use tracing_subscriber::{Layer, layer::Context, prelude::*};
use uuid::Uuid;

type Response = (StatusCode, HeaderMap, Value);

#[derive(Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Armed,
    Held,
    Released,
    Returned,
}

#[derive(Default)]
struct PauseState {
    phase: Mutex<Phase>,
    entered: Notify,
    changed: Condvar,
}

#[derive(Clone, Default)]
struct TicketPause(Arc<PauseState>);

impl<S: Subscriber> Layer<S> for TicketPause {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() != "hito_core::credential_ticket_test" {
            return;
        }
        struct Point(bool);
        impl tracing::field::Visit for Point {
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "point" && value == "credential_ticket_before_lock" {
                    self.0 = true;
                }
            }
        }
        let mut point = Point(false);
        event.record(&mut point);
        if !point.0 {
            return;
        }
        let mut phase = self.0.phase.lock().unwrap();
        if *phase != Phase::Armed {
            return;
        }
        *phase = Phase::Held;
        drop(phase);
        tokio::task::block_in_place(|| {
            self.0.entered.notify_one();
            let phase = self.0.phase.lock().unwrap();
            let (mut phase, timed) = self
                .0
                .changed
                .wait_timeout_while(phase, Duration::from_secs(4), |phase| *phase == Phase::Held)
                .unwrap();
            assert!(
                !timed.timed_out() && *phase == Phase::Released,
                "The owned pre-lock ticket pause must be explicitly released"
            );
            *phase = Phase::Returned;
        });
    }
}

impl TicketPause {
    async fn entered(&self) {
        self.0.entered.notified().await;
        self.assert_held();
    }

    fn assert_held(&self) {
        assert_eq!(*self.0.phase.lock().unwrap(), Phase::Held);
    }

    fn release(&self) {
        let mut phase = self.0.phase.lock().unwrap();
        if matches!(*phase, Phase::Armed | Phase::Held) {
            *phase = Phase::Released;
            self.0.changed.notify_all();
        }
    }

    fn assert_returned(&self) {
        assert_eq!(*self.0.phase.lock().unwrap(), Phase::Returned);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_success_dispatched_after_a_prelock_pause_clears_the_intervening_rejection() {
    let primary = fixture::ProviderServer::start().await;
    let previous = fixture::ProviderServer::start().await;
    let mut plugin =
        FakePlugin::builder(Manifest {
            id: "org.hito.prelock-credential-ordering".into(),
            name: "Owned pre-lock credential service".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [
                Capability::Search,
                Capability::Sources,
                Capability::Metadata,
                Capability::Lyrics,
            ]
            .into(),
            authorization: None,
            content_policy: None,
        })
        .require_protocol()
        .get(HEALTH_PATH, |_| {
            Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
        })
        .post(SEARCH_PATH, |request| {
            assert!(request.headers.get("authorization").is_some_and(|value| {
                value == format!("Bearer {}", fixture::OWNER_TOKEN).as_str()
            }));
            assert!(request.query.is_none());
            let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
            assert!(search.validate().is_ok());
            assert_eq!(search.limit, 20);
            assert!(search.cursor.is_none());
            match search.query.as_str() {
                "Owned pre-lock successful query" => {
                    Reply::Json(json!({"items":[],"next_cursor":null}))
                }
                "Owned performer Owned tune" => Reply::Status(StatusCode::UNAUTHORIZED),
                _ => panic!("The owned provider only accepts the two ordering requests"),
            }
        })
        .start()
        .await;
    let pause = TicketPause::default();
    let mut requests = JoinSet::new();
    let mut context = None;
    let result = AssertUnwindSafe(hang_guard(
        "owned pre-lock credential dispatch ordering",
        Box::pin(exercise(
            &primary,
            &previous,
            &plugin,
            &pause,
            &mut context,
            &mut requests,
        )),
    ))
    .catch_unwind()
    .await;
    pause.release();
    requests.abort_all();
    let drained = tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await;
    let stopped = plugin.finish().await;
    if let Some(context) = &context {
        context.owned.assert_preserved().await;
    }
    primary.shutdown().await;
    previous.shutdown().await;
    assert!(drained.is_ok(), "All owned ordering HTTP tasks must join");
    assert!(stopped, "The owned ordering provider must join");
    let (current, expected) = match result {
        Ok(value) => value,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        current["health"], expected,
        "A checked success dispatched after the intervening rejection must clear private credential health even when its timestamp was captured before the ticket lock"
    );
}

async fn exercise(
    primary: &fixture::ProviderServer,
    previous: &fixture::ProviderServer,
    plugin: &FakePlugin,
    pause: &TicketPause,
    context_owner: &mut Option<fixture::Context>,
    requests: &mut JoinSet<Response>,
) -> (Value, Value) {
    let context = context_owner.insert(
        hang_guard(
            "owned credential ordering setup",
            Box::pin(fixture::Context::prepare(primary, previous, plugin)),
        )
        .await,
    );
    let expected =
        fixture::installation_by_id(&context.before, &context.installation["id"])["health"].clone();
    assert_eq!(expected["state"], "ok");
    assert_eq!(plugin.health_requests().len(), 2);
    let path = format!(
        "/api/v1/recordings/{}/lyrics/candidates",
        context.owned.recording
    );
    let request = json!({
        "installation_id":context.installation["id"],
        "installation_revision":context.installation["revision"],
        "query":"Owned pre-lock successful query",
    });
    let app = context.app.clone();
    let headers = context.owner.headers();
    hito_testkit::logs::anchor_callsite_interest();
    let subscriber = tracing_subscriber::registry().with(pause.clone());
    requests.spawn(
        async move { account::exchange(&app, "POST", &path, request, headers).await }
            .with_subscriber(subscriber),
    );
    pause.entered().await;
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    assert!(requests.try_join_next().is_none());
    let request = json!({"request_id":Uuid::new_v4()});
    let rejection = context.resolve(&request).await;
    context.check_resolution(&rejection, &request);
    assert_eq!(rejection.2["outcome"], "provider_unavailable");
    context.check_replay(&request, &rejection.2).await;
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(
        plugin.requests_for(SEARCH_PATH)[0].json()["query"],
        "Owned performer Owned tune"
    );
    let rejected = context.settings().await;
    assert_eq!(
        fixture::installation_by_id(&rejected, &context.installation["id"])["health"]["state"],
        "credentials_required"
    );
    pause.assert_held();
    assert!(requests.try_join_next().is_none());
    pause.release();
    let (status, headers, response) =
        tokio::time::timeout(Duration::from_secs(3), requests.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(response, json!({"items":[],"next_cursor":null}));
    pause.assert_returned();
    assert!(requests.is_empty());
    let recovered = context
        .check_preserved(
            &context.before,
            &context.choice,
            primary,
            previous,
            plugin,
            &rejection.2,
        )
        .await;
    let current = fixture::installation_by_id(&recovered, &context.installation["id"]).clone();
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 2);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 2);
    assert_eq!(
        plugin
            .requests()
            .into_iter()
            .map(|request| request.operation)
            .collect::<Vec<_>>(),
        [MANIFEST_PATH, MANIFEST_PATH, SEARCH_PATH, SEARCH_PATH]
    );
    assert_eq!(
        plugin.requests_for(SEARCH_PATH)[1].json()["query"],
        "Owned pre-lock successful query"
    );
    assert_eq!(plugin.unexpected(), 0);
    (current, expected)
}
