use super::{
    fixture::{Host, Library},
    http,
};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use hito_core::{Actor, Browse, ReplicaSession, Replicas};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot};

#[derive(Clone)]
struct DependentRequest {
    replicas: Replicas,
    actor: Actor,
    session: ReplicaSession,
    retired: Arc<AtomicBool>,
}

async fn depend_on_replica(
    State(state): State<DependentRequest>,
    request: Request,
    next: Next,
) -> Response {
    let me = request.uri().path() == "/api/v1/me";
    let response = next.run(request).await;
    if me && response.status().is_success() {
        let result = state
            .replicas
            .synchronize(&state.actor, &state.session)
            .await;
        state.retired.store(
            matches!(result, Err(hito_core::Error::Conflict(_))),
            Ordering::SeqCst,
        );
    }
    response
}

#[tokio::test]
async fn shared_server_shutdown_closes_replicas_while_http_drains_and_rejects_stale_use() {
    let remote = Host::start().await;
    let local = Library::new("owned-runtime-replica-client").await;
    let connection = local
        .services
        .replicas
        .connect(&local.actor, remote.request("admin"))
        .await
        .unwrap();
    let session = local
        .services
        .replicas
        .activate(&local.actor, connection.id)
        .await
        .unwrap();
    let retired = Arc::new(AtomicBool::new(false));
    let app =
        hito_server::router(local.services.clone()).layer(axum::middleware::from_fn_with_state(
            DependentRequest {
                replicas: local.services.replicas.clone(),
                actor: local.actor.clone(),
                session: session.clone(),
                retired: retired.clone(),
            },
            depend_on_replica,
        ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let mut serving = tokio::spawn(hito_server::serve(
        listener,
        app,
        local.services.clone(),
        async {
            let _ = stopped.await;
        },
    ));
    assert!(
        http::get(address, &local.token, "/api/v1/playlists")
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
    remote.faults.hold_next_pull();
    let token = local.token.clone();
    let request =
        tokio::spawn(
            async move { http::call(address, "GET", "/api/v1/me", None, Some(&token)).await },
        );
    remote.faults.held().await;
    stop.send(()).unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(2), &mut serving).await;
    let joined_before_cleanup = joined.is_ok();
    let closed_before_cleanup = local
        .services
        .replicas
        .playlists(&local.actor, &session, Browse::default())
        .await
        .is_err();
    let retired_before_cleanup = retired.load(Ordering::SeqCst);
    local.services.replicas.close().await.unwrap();
    let completed = match joined {
        Ok(result) => result.unwrap().is_ok(),
        Err(_) => tokio::time::timeout(Duration::from_secs(3), &mut serving)
            .await
            .unwrap()
            .unwrap()
            .is_ok(),
    };
    remote.faults.release();
    let response = request.await.unwrap();
    remote.close().await;
    assert!(completed, "shared server failed its joined shutdown");
    assert!(
        joined_before_cleanup,
        "replica retirement waited behind the HTTP drain instead of releasing it"
    );
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "HTTP drain cancelled the dependent response"
    );
    assert_eq!(response.headers["x-content-type-options"], "nosniff");
    assert_eq!(
        response.json()["id"].as_str(),
        Some(local.actor.user().id.as_str()),
        "HTTP drain changed the authenticated response"
    );
    assert!(
        retired_before_cleanup,
        "HTTP drain did not receive replica retirement before host shutdown completion"
    );
    assert!(
        closed_before_cleanup,
        "shutdown returned with an active replica owner"
    );
}
