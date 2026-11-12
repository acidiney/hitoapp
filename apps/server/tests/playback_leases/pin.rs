use super::fixture::{Case, credentials};
use axum::http::StatusCode;
use hito_core::{PlaybackLease, SetPin, Unlock};
use hito_testkit::logs::LogCapture;
use http_body_util::BodyExt;
use tracing::instrument::WithSubscriber;

const OPERATIONS: [(&str, &str, &str); 4] = [
    ("GET", "", "lease_status"),
    ("GET", "/audio", "open_lease_audio"),
    ("HEAD", "/audio", "open_lease_audio"),
    ("DELETE", "", "release_lease"),
];

async fn set_pin(f: &Case) {
    f.services
        .identity
        .set_pin(
            &f.actor,
            SetPin {
                password: credentials().password,
                pin: Some("123456".into()),
                idle_minutes: 0,
            },
        )
        .await
        .unwrap();
}

async fn lease(f: &Case) -> PlaybackLease {
    let response = f.begin().await;
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn locked_lifecycle_requests_reach_core_and_old_leases_stay_retired_after_unlock() {
    let mut evidence = Vec::new();
    for (method, suffix, core_span) in OPERATIONS {
        let f = Case::new().await;
        set_pin(&f).await;
        let lease = lease(&f).await;
        let root = format!("/api/v1/playback/leases/{}", lease.id);
        let locked = f
            .request("POST", "/api/v1/account/lock", &[], None)
            .await
            .status();
        let logs = LogCapture::default();
        let response = f
            .request(method, &format!("{root}{suffix}"), &[], None)
            .with_subscriber(
                logs.dispatch_with_spans("off,hito_server=trace,hito_core::playback_access=trace"),
            )
            .await;
        let status = response.status();
        let request_id = response.headers()["x-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        drop(response);
        let delegated = logs.events().iter().any(|event| {
            event["fields"]["error"] == format!("{:?}", hito_core::Error::Locked)
                && event["spans"].as_array().is_some_and(|spans| {
                    spans.iter().any(|span| span["name"] == core_span)
                        && spans.iter().any(|span| {
                            span["name"] == "http_request" && span["request_id"] == request_id
                        })
                })
        });
        f.services
            .identity
            .unlock(
                &f.actor,
                Unlock {
                    pin: "123456".into(),
                },
            )
            .await
            .unwrap();
        let terminal = f.request("GET", &root, &[], None).await.status();
        let fresh = f.begin().await.status();
        logs.assert_excludes(&[
            &f.token,
            f.directory.path().to_str().unwrap(),
            "private-owned.wav",
        ]);
        f.close().await;
        evidence.push((method, suffix, locked, status, delegated, terminal, fresh));
    }
    assert!(
        evidence
            .iter()
            .all(
                |(_, _, lock, status, delegated, terminal, fresh)| *lock == StatusCode::OK
                    && *status == StatusCode::LOCKED
                    && *delegated
                    && [StatusCode::NOT_FOUND, StatusCode::CONFLICT].contains(terminal)
                    && *fresh == StatusCode::OK
            ),
        "Locked HTTP requests must delegate to Core, retaining request correlation: {evidence:?}"
    );
}

#[tokio::test]
async fn a_second_locked_session_cannot_retire_the_owner_through_the_router() {
    let mut evidence = Vec::new();
    for (method, suffix, _) in OPERATIONS {
        let f = Case::new().await;
        set_pin(&f).await;
        let lease = lease(&f).await;
        let root = format!("/api/v1/playback/leases/{}", lease.id);
        let session = f.services.identity.login(credentials()).await.unwrap();
        let other = f
            .services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        f.services.identity.lock(&other).await.unwrap();
        let foreign_locked = f
            .request_as(
                Some(&session.token),
                method,
                &format!("{root}{suffix}"),
                &[],
                None,
            )
            .await
            .status();
        f.services
            .identity
            .unlock(
                &other,
                Unlock {
                    pin: "123456".into(),
                },
            )
            .await
            .unwrap();
        let foreign = f
            .request_as(
                Some(&session.token),
                method,
                &format!("{root}{suffix}"),
                &[],
                None,
            )
            .await
            .status();
        let owner = f.request("GET", &root, &[], None).await.status();
        f.close().await;
        evidence.push((method, suffix, foreign_locked, foreign, owner));
    }
    assert!(
        evidence.iter().all(
            |(_, _, locked, foreign, owner)| *locked == StatusCode::LOCKED
                && *foreign == StatusCode::NOT_FOUND
                && *owner == StatusCode::OK
        ),
        "Another session must not retire or obtain the lease: {evidence:?}"
    );
}
