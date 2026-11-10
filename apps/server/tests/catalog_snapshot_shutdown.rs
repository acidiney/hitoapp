#[path = "fixtures/artwork_http.rs"]
mod http;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use hito_core::{Actor, CatalogReplication, Credentials, test_support::support};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Notify, oneshot},
};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    instrument::WithSubscriber,
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};
use uuid::Uuid;

#[derive(Clone)]
struct DependentRequest {
    snapshots: CatalogReplication,
    actor: Actor,
    id: Uuid,
    entered: Arc<Notify>,
    closed: Arc<Notify>,
    unavailable: Arc<AtomicBool>,
}

async fn wait_for_snapshot_retirement(
    State(state): State<DependentRequest>,
    request: Request,
    next: Next,
) -> Response {
    let me = request.uri().path() == "/api/v1/me";
    let response = next.run(request).await;
    if me && response.status().is_success() {
        state.entered.notify_one();
        state.closed.notified().await;
        state.unavailable.store(
            state
                .snapshots
                .page(&state.actor, state.id, 0)
                .await
                .is_err(),
            Ordering::SeqCst,
        );
    }
    response
}

struct ClosedEvent(Arc<Notify>);
impl<S: Subscriber> Layer<S> for ClosedEvent {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        struct Visitor<'a>(&'a Notify);
        impl Visit for Visitor<'_> {
            fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "event" && value == "catalog_snapshots_closed" {
                    self.0.notify_one();
                }
            }
        }
        event.record(&mut Visitor(&self.0));
    }
}

#[tokio::test]
async fn shared_shutdown_retires_snapshots_while_http_is_still_draining() {
    let (_dir, services, actor, library, music) = support::library("owned-snapshot-runtime").await;
    support::write_wav(&music.join("owned.wav"), 0);
    support::scan(&services, &actor, &library).await;
    let token = services
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap()
        .token;
    let snapshot = services
        .catalog_replication
        .begin(&actor, &library)
        .await
        .unwrap();
    let entered = Arc::new(Notify::new());
    let closed = Arc::new(Notify::new());
    let unavailable = Arc::new(AtomicBool::new(false));
    let app = hito_server::router(services.clone()).layer(axum::middleware::from_fn_with_state(
        DependentRequest {
            snapshots: services.catalog_replication.clone(),
            actor: actor.clone(),
            id: snapshot.id,
            entered: entered.clone(),
            closed: closed.clone(),
            unavailable: unavailable.clone(),
        },
        wait_for_snapshot_retirement,
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(ClosedEvent(closed)));
    let mut serving = tokio::spawn(
        hito_server::serve(listener, app, services.clone(), async {
            let _ = stopped.await;
        })
        .with_subscriber(dispatch.clone()),
    );
    assert!(
        http::get(address, &token, "/api/v1/playlists")
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
    let request =
        tokio::spawn(
            async move { http::call(address, "GET", "/api/v1/me", None, Some(&token)).await },
        );
    let ready = tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .is_ok();
    let _ = stop.send(());
    let joined = tokio::time::timeout(Duration::from_secs(2), &mut serving).await;
    let joined_before_cleanup = joined.is_ok();
    let unavailable_before_cleanup = unavailable.load(Ordering::SeqCst);
    let completed = match joined {
        Ok(result) => result.unwrap().is_ok(),
        Err(_) => {
            services
                .catalog_replication
                .close()
                .with_subscriber(dispatch)
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(3), &mut serving)
                .await
                .unwrap()
                .unwrap()
                .is_ok()
        }
    };
    let request_result = request.await;
    let request_error = request_result.as_ref().err();
    let request_status = request_result.as_ref().ok().map(|response| response.status);
    assert!(completed);
    assert!(
        ready,
        "the dependent request did not reach its shutdown barrier: status={request_status:?}, task_error={request_error:?}"
    );
    let response = request_result.expect("the owned request task must finish after teardown");
    assert!(
        joined_before_cleanup,
        "snapshot retirement waited behind the HTTP drain"
    );
    assert!(
        unavailable_before_cleanup,
        "HTTP drain must observe retired snapshots before shutdown completes"
    );
    assert_eq!(response.status, axum::http::StatusCode::OK);
    assert_eq!(response.headers["x-content-type-options"], "nosniff");
    assert_eq!(
        response.json()["id"].as_str(),
        Some(actor.user().id.as_str())
    );
    assert!(
        services
            .catalog_replication
            .page(&actor, snapshot.id, 0)
            .await
            .is_err()
    );
}
