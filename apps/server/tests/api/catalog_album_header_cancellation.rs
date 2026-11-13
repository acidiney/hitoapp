use super::{Reviewed, prepare, private};
use crate::{
    artist_review_fixture as review, catalog_album_header_fixture as catalog,
    provider_fixture::ProviderServer, support,
};
use axum::{Router, http::StatusCode};
use futures_util::FutureExt;
use hito_core::{
    CoreConfig, Credentials, HostDeviceId, ReadReleaseGroup, ReleaseGroupAvailability, Services,
    test_support::{database_owner::DatabaseOwner, hang_guard::hang_guard},
};
use hito_testkit::fake_plugin::{FakePlugin, Gate};
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinSet, time::timeout};

struct InFlightGroup {
    reads: JoinSet<hito_core::Result<ReleaseGroupAvailability>>,
    gate: Option<Arc<Gate>>,
}

#[tokio::test]
async fn cancelling_a_stalled_group_retires_its_body_and_releases_the_same_accounts_admission() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let audio = music.join("owned-cancelled-group-original.wav");
    support::write_wav(&audio);
    let original = std::fs::read(&audio).unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("cancelled-group-http").unwrap(),
    );
    owner.attach(&mut config);
    let services = hang_guard(
        "cancelled group owned services",
        Box::pin(hito_core::open(config)),
    )
    .await
    .unwrap();
    let app = hito_server::router(services.clone());
    let origin = hang_guard(
        "cancelled group owned origin",
        Box::pin(ProviderServer::start()),
    )
    .await;
    let mut provider =
        hang_guard("cancelled group owned catalog", Box::pin(catalog::start())).await;
    let mut in_flight = InFlightGroup {
        reads: JoinSet::new(),
        gate: None,
    };
    let outcome = AssertUnwindSafe(Box::pin(async {
        let reviewed = hang_guard(
            "cancelled group reviewed association",
            Box::pin(prepare(&app, &origin, &provider)),
        )
        .await;
        let session = hang_guard(
            "cancelled group owned session",
            Box::pin(services.identity.login(Credentials {
                username: "owner".into(),
                password: "owned artist review administrator password".into(),
            })),
        )
        .await
        .unwrap();
        let actor = services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        assert_eq!(session.user.id, reviewed.account.id);
        assert_eq!(actor.user().id, session.user.id);
        timeout(
            Duration::from_secs(30),
            Box::pin(cancel_and_read_again(
                &app,
                &services,
                &actor,
                &reviewed,
                &origin,
                &provider,
                &mut in_flight,
            )),
        )
        .await
        .expect("The explicit group cancellation workflow must finish within its deadline");
    }))
    .catch_unwind()
    .await;
    if let Some(gate) = &in_flight.gate {
        gate.open();
    }
    in_flight.reads.shutdown().await;
    let joined = provider.finish().await;
    origin.shutdown().await;
    let playback_closed = services.playback.close().await;
    let replicas_closed = services.replicas.close().await;
    let replication_closed = services.catalog_replication.close().await;
    drop(app);
    drop(services);
    let retired = owner.retire(Duration::from_secs(5)).await;
    assert!(joined);
    assert!(playback_closed.is_ok() && replicas_closed.is_ok() && replication_closed.is_ok());
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    assert_eq!(owner.observe().active_resources(), 0);
    assert_eq!(std::fs::read(audio).unwrap(), original);
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

async fn cancel_and_read_again(
    app: &Router,
    services: &Services,
    actor: &hito_core::Actor,
    reviewed: &Reviewed,
    origin: &ProviderServer,
    provider: &FakePlugin,
    in_flight: &mut InFlightGroup,
) {
    let Reviewed {
        account,
        peer,
        imported,
        association,
    } = reviewed;
    let before = imported.snapshot(app, account).await;
    let origin_before = review::origin_counts(origin).await;
    let peer_before = peer.get(app, "/api/v1/recordings").await;
    let requests_before = provider.requests().len();
    let association_id = association["id"].as_str().unwrap();
    let path = format!(
        "/api/v1/recordings/{}/artist-associations/{association_id}/release-group",
        imported.recording,
    );
    let (status, headers, _) = review::exchange(
        app,
        "POST",
        &path,
        json!({"release_group_id": catalog::GROUP}),
        peer.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    private(&headers);
    assert_eq!(provider.requests().len(), requests_before);
    let gate = provider.stall_body(catalog::GROUP_DETAIL, 1);
    in_flight.gate = Some(gate.clone());
    let (cancel, cancelled) = oneshot::channel();
    let catalog = services.artist_catalog.clone();
    let request_actor = actor.clone();
    let recording = imported.recording.clone();
    let association_id = association_id.to_owned();
    in_flight.reads.spawn(async move {
        catalog
            .recording_artist_release_group_until(
                &request_actor,
                &recording,
                &association_id,
                ReadReleaseGroup {
                    release_group_id: catalog::GROUP.into(),
                },
                async {
                    let _ = cancelled.await;
                },
            )
            .await
    });
    assert!(
        gate.entered_within(Duration::from_secs(3)).await,
        "The exact detail body must be stalled before cancellation"
    );
    assert_eq!(in_flight.reads.len(), 1);
    cancel
        .send(())
        .expect("The cancellation receiver must remain owned");
    let cancelled = timeout(Duration::from_secs(3), in_flight.reads.join_next())
        .await
        .expect("Cancellation must finish before the stalled body is released")
        .expect("The cancelled group read must remain owned")
        .expect("The cancelled group read must not panic");
    assert!(matches!(cancelled, Ok(ReleaseGroupAvailability::Cancelled)));
    assert!(in_flight.reads.is_empty());
    assert_eq!(provider.requests().len(), requests_before + 2);
    provider.clear_fault(catalog::GROUP_DETAIL);
    let fresh = timeout(
        Duration::from_secs(3),
        services
            .artist_catalog
            .recording_artist_release_group_until(
                actor,
                &imported.recording,
                association["id"].as_str().unwrap(),
                ReadReleaseGroup {
                    release_group_id: catalog::GROUP.into(),
                },
                std::future::pending(),
            ),
    )
    .await
    .expect("The same account must be admitted before the old gate opens")
    .unwrap();
    assert_eq!(
        serde_json::to_value(fresh).unwrap(),
        json!({"status": "available", "association_id": association["id"], "group": catalog::group()}),
    );
    assert!(
        gate.retired_within(Duration::from_secs(3)).await,
        "The cancelled body must retire before the old gate opens"
    );
    gate.open();
    assert_eq!(imported.snapshot(app, account).await, before);
    assert_eq!(review::origin_counts(origin).await, origin_before);
    assert_eq!(peer.get(app, "/api/v1/recordings").await, peer_before);
    assert_eq!(provider.unexpected(), 0);
    let requests = provider.requests();
    let dispatched = &requests[requests_before..];
    assert_eq!(
        dispatched
            .iter()
            .map(|request| request.operation.as_str())
            .collect::<Vec<_>>(),
        [
            catalog::GROUP_CAPABILITIES,
            catalog::GROUP_DETAIL,
            catalog::GROUP_CAPABILITIES,
            catalog::GROUP_DETAIL,
        ],
    );
    assert!(
        dispatched
            .iter()
            .all(|request| request.protocol().as_deref() == Some("1"))
    );
    for request in dispatched
        .iter()
        .filter(|request| request.operation == catalog::GROUP_DETAIL)
    {
        assert_eq!(request.json(), json!({"release_group_id": catalog::GROUP}));
    }
}
