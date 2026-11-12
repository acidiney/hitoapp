use super::setup::Prepared;
use crate::account;
use axum::http::StatusCode;
use hito_core::{Action, Operation};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone, Copy)]
enum Change {
    GrantEpoch,
    PersonalMetadata,
    Installation,
    Service,
    Session,
    PinConfiguration,
    PinLock,
    Identity,
}

async fn set_pin(prepared: &Prepared) {
    let (status, _) = prepared.library.account.request(&prepared.library.app, "PUT", "/api/v1/account/pin", json!({
        "password":"owned artist review administrator password", "pin":"4271", "idle_minutes":0,
    })).await;
    assert_eq!(status, StatusCode::OK);
}

async fn change(prepared: &Prepared, change: Change) -> Vec<StatusCode> {
    let library = &prepared.library;
    let account = &library.account;
    let app = &library.app;
    match change {
        Change::GrantEpoch => {
            let libraries = account.get(app, "/api/v1/libraries").await;
            let shared = libraries
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["personal"] == false)
                .unwrap()["id"]
                .as_str()
                .unwrap();
            let path = format!("/api/v1/libraries/{shared}/members/{}", account.id);
            for method in ["DELETE", "PUT"] {
                let (status, _) = account.request(app, method, &path, Value::Null).await;
                assert_eq!(status, StatusCode::NO_CONTENT);
            }
            vec![StatusCode::CONFLICT, StatusCode::NOT_FOUND]
        }
        Change::PersonalMetadata => {
            let operation = Operation {
                id: Uuid::new_v4(),
                action: Action::RecordingTitleSet {
                    recording_id: Uuid::parse_str(&library.recording).unwrap(),
                    title: "Owned personal lyric title change".into(),
                },
            };
            let (status, _) = account
                .request(
                    app,
                    "POST",
                    "/api/v1/sync",
                    json!({"device_id":"owned-lyric-authority-client","operations":[operation]}),
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            vec![StatusCode::CONFLICT]
        }
        Change::Installation => {
            let (status, _) = account
                .request(
                    app,
                    "DELETE",
                    &format!(
                        "/api/v1/plugins/{}/credential",
                        prepared.installation["id"].as_str().unwrap()
                    ),
                    json!({"revision":prepared.installation["revision"]}),
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            vec![StatusCode::CONFLICT]
        }
        Change::Service => {
            let (status, _) = account
                .request(
                    app,
                    "DELETE",
                    &format!(
                        "/api/v1/plugins/services/{}",
                        prepared.installation["service_id"].as_str().unwrap()
                    ),
                    json!({"revision":1}),
                )
                .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            vec![StatusCode::NOT_FOUND]
        }
        Change::Session => {
            let (status, _) = account
                .request(app, "POST", "/api/v1/logout", Value::Null)
                .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            vec![StatusCode::UNAUTHORIZED]
        }
        Change::PinConfiguration => {
            set_pin(prepared).await;
            vec![StatusCode::CONFLICT]
        }
        Change::PinLock => {
            let (status, _) = account
                .request(app, "POST", "/api/v1/account/lock", Value::Null)
                .await;
            assert_eq!(status, StatusCode::OK);
            vec![StatusCode::LOCKED]
        }
        Change::Identity => {
            let (status, _) = account
                .request(
                    app,
                    "DELETE",
                    &format!(
                        "{}/identities/{}?revision=1",
                        prepared.base(),
                        prepared.identity["selector"]["identity_id"]
                            .as_str()
                            .unwrap()
                    ),
                    Value::Null,
                )
                .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            vec![StatusCode::NOT_FOUND]
        }
    }
}

async fn held(change_kind: Change, workflow: &str, call: &'static str) {
    let prepared = Prepared::new().await;
    if matches!(change_kind, Change::PinLock) {
        set_pin(&prepared).await;
    }
    let gate = prepared.provider.hold(call);
    let body = match workflow {
        "identities/review" => prepared.candidate.clone(),
        "identities" => prepared.accept.clone(),
        _ => prepared.read_body(),
    };
    let path = format!("{}/{workflow}", prepared.base());
    let app = prepared.library.app.clone();
    let headers = prepared.library.account.headers();
    let request =
        tokio::spawn(async move { account::exchange(&app, "POST", &path, body, headers).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("The provider must enter its held body");
    let expected = change(&prepared, change_kind).await;
    let (status, _, result) = tokio::time::timeout(Duration::from_secs(3), request)
        .await
        .expect("Authority loss must retire held work promptly")
        .unwrap();
    assert!(
        expected.contains(&status),
        "Held lyric work must reject changed authority"
    );
    assert!(result.get("document").is_none() && result.get("review_stamp").is_none());
    tokio::time::timeout(Duration::from_secs(2), gate.retired())
        .await
        .expect("Authority loss must close the provider body before it is released");
    assert!(!gate.released());
    prepared.library.assert_original();
    prepared.provider.finish().await;
}

#[tokio::test]
async fn held_metadata_reviews_and_accepts_recheck_current_private_recording_and_plugin_authority()
{
    tokio::time::timeout(Duration::from_secs(60), async {
        for workflow in ["identities/review", "identities"] {
            for change in [
                Change::GrantEpoch,
                Change::PersonalMetadata,
                Change::Installation,
                Change::Service,
                Change::Session,
                Change::PinConfiguration,
            ] {
                held(change, workflow, "metadata").await;
            }
        }
    })
    .await
    .expect("Owned held metadata cases must finish within their deadline");
}

#[tokio::test]
async fn held_selected_reads_recheck_authority_and_retained_identity_before_publication() {
    tokio::time::timeout(Duration::from_secs(60), async {
        for change in [
            Change::GrantEpoch,
            Change::PersonalMetadata,
            Change::Installation,
            Change::Service,
            Change::Session,
            Change::PinConfiguration,
            Change::PinLock,
            Change::Identity,
        ] {
            held(change, "identities/read", "v2_lyrics").await;
        }
    })
    .await
    .expect("Owned held lyric cases must finish within their deadline");
}

#[tokio::test]
async fn review_stamps_reject_another_session_and_changed_metadata_before_acceptance() {
    let prepared = Prepared::new().await;
    let another = account::Account::login(
        &prepared.library.app,
        "owner",
        "owned artist review administrator password",
    )
    .await;
    let (status, _) = another
        .request(
            &prepared.library.app,
            "POST",
            &format!("{}/identities", prepared.base()),
            prepared.accept.clone(),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "A review belongs to its exact session"
    );
    prepared
        .provider
        .metadata_title("Changed provider metadata with the same reference")
        .await;
    let (status, _) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{}/identities", prepared.base()),
            prepared.accept.clone(),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "Accept must refetch and compare the exact reviewed metadata"
    );
    prepared.provider.metadata_revision("independent:2").await;
    let (status, _) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{}/identities", prepared.base()),
            prepared.accept.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let list = prepared
        .library
        .account
        .get(
            &prepared.library.app,
            &format!("{}/identities", prepared.base()),
        )
        .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    prepared.library.assert_original();
    prepared.provider.finish().await;
}

#[tokio::test]
async fn changing_only_the_current_pin_idle_policy_retires_a_held_lyric_read() {
    let prepared = Prepared::new().await;
    set_pin(&prepared).await;
    let gate = prepared.provider.hold("v2_lyrics");
    let app = prepared.library.app.clone();
    let path = format!("{}/identities/read", prepared.base());
    let body = prepared.read_body();
    let headers = prepared.library.account.headers();
    let mut task =
        tokio::spawn(async move { account::exchange(&app, "POST", &path, body, headers).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    let (status,_)=prepared.library.account.request(&prepared.library.app,"PUT","/api/v1/account/pin",json!({"password":"owned artist review administrator password","pin":null,"idle_minutes":5})).await;
    assert_eq!(status, StatusCode::OK);
    let completed = tokio::time::timeout(Duration::from_secs(2), &mut task).await;
    let rejected = matches!(&completed, Ok(Ok((StatusCode::CONFLICT, _, _))));
    if completed.is_err() {
        task.abort();
        let _ = task.await;
    }
    tokio::time::timeout(Duration::from_secs(2), gate.retired())
        .await
        .unwrap();
    assert!(!gate.released());
    prepared.library.assert_original();
    prepared.provider.finish().await;
    assert!(
        rejected,
        "A current PIN idle-policy change must invalidate held lyric authority even when the PIN hash stays unchanged"
    );
}
