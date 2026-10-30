#![cfg(unix)]

use hito_core::test_support::community_recovery as fixture;
use hito_core::test_support::community_service as community;
#[path = "../fixtures/artwork_http.rs"]
#[allow(dead_code)]
mod http;

use futures_util::FutureExt;
use hito_core::{CommunityReportAdmission, SuccessfulPlayback, UpdateCommunityPreferences};
use std::{future::pending, panic::AssertUnwindSafe, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};
use uuid::Uuid;

#[tokio::test]
async fn the_host_reports_in_the_background_and_joins_a_held_delivery_without_blocking_http() {
    let remote = community::CommunityServer::start().await;
    let (library, hito) = fixture::Library::prepare(&remote).await;
    let owner = &library.accounts[0];
    hito.community
        .update_preferences(
            &owner.actor,
            UpdateCommunityPreferences {
                revision: 0,
                playback_reporting: true,
                metadata_corrections: false,
            },
        )
        .await
        .unwrap();
    let submission_id = Uuid::new_v4();
    assert_eq!(
        hito.community
            .observe_success(
                &owner.actor,
                &owner.source.id,
                SuccessfulPlayback { submission_id }
            )
            .await
            .unwrap(),
        CommunityReportAdmission::Queued
    );
    assert!(remote.requests().await.is_empty());
    let before = owner.private_state(&hito).await;
    let held = remote.hold_outcome().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(hito_server::serve(
        listener,
        hito_server::router_with_config(
            hito.clone(),
            hito_server::Config {
                origin: format!("http://{address}"),
                ..Default::default()
            },
        ),
        hito.clone(),
        async {
            let _ = stopped.await;
        },
    ));
    let outcome = AssertUnwindSafe(async {
        held.entered().await;
        assert_eq!(remote.requests().await.len(), 2);
        assert_eq!(
            http::call(address, "GET", "/health", None, None)
                .await
                .status,
            axum::http::StatusCode::OK
        );
        assert_eq!(remote.accepted_receipts().await.len(), 1);
    })
    .catch_unwind()
    .await;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(3), serving)
        .await
        .expect("Shutdown must cancel and join held community delivery")
        .unwrap()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    held.release();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    let retained = remote.accepted_receipts().await;
    assert_eq!(owner.private_state(&hito).await, before);
    drop(hito);
    let hito = library.open(&remote).await;
    assert!(
        hito.community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    assert_eq!(remote.requests().await.len(), 3);
    assert_eq!(remote.accepted_receipts().await, retained);
    assert_eq!(
        hito.community
            .observe_success(
                &owner.actor,
                &owner.source.id,
                SuccessfulPlayback { submission_id }
            )
            .await
            .unwrap(),
        CommunityReportAdmission::AlreadyAccepted
    );
    assert_eq!(owner.private_state(&hito).await, before);
    library.assert_originals();
}
