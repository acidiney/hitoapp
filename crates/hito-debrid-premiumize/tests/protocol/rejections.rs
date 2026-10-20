use super::*;
use hito_debrid_premiumize::ProviderRejection;

#[tokio::test]
async fn provider_error_codes_are_typed_without_returning_private_messages() {
    let cases = [
        (
            "authentication_failed",
            ProviderRejection::AuthenticationFailed,
        ),
        ("permission_denied", ProviderRejection::PermissionDenied),
        ("rate_limit_reached", ProviderRejection::RateLimited),
        ("not_found", ProviderRejection::NotFound),
        ("future_private_provider_code", ProviderRejection::Rejected),
    ];
    let fixture = Fixture::start(
        cases
            .iter()
            .map(|(code, _)| {
                serde_json::to_vec(&json!({
            "status": "error", "code": code,
            "message": "private provider message with https://private.invalid/signed?token=private",
        })).unwrap()
            })
            .collect(),
    )
    .await;
    let client = client(&fixture);
    let capture = Capture::default();
    for (_, expected) in cases {
        let result = client
            .observe(&binding())
            .with_subscriber(capture.dispatch())
            .await;
        assert!(matches!(result, Err(Error::ProviderRejected(actual)) if actual == expected));
    }
    assert_eq!(fixture.close().await.len(), 5);
    capture.assert_workflows(
        &[("provider_rejected", 0); 5],
        &[TOKEN, TRANSFER_ID, "private"],
    );
}

#[tokio::test]
async fn malformed_envelopes_fields_and_unknown_phases_never_qualify_completion() {
    let mut bodies = vec![b"private non-JSON response".to_vec()];
    for mutation in 0..6 {
        let mut value = transfer("finished", 1.0);
        match mutation {
            0 => value["transfers"] = Value::Null,
            1 => {
                value["transfers"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("created_at");
            }
            2 => value["transfers"][0]["created_at"] = json!(-1),
            3 => value["transfers"][0]["progress"] = json!("private invalid fraction"),
            4 => value["code"] = json!("authentication_failed"),
            5 => value["transfers"][0]["status"] = json!("future_private_phase"),
            _ => unreachable!(),
        }
        bodies.push(serde_json::to_vec(&value).unwrap());
    }
    let fixture = Fixture::start(bodies).await;
    let client = client(&fixture);
    let capture = Capture::default();
    for index in 0..7 {
        let result = client
            .observe(&binding())
            .with_subscriber(capture.dispatch())
            .await;
        let expected = if index == 6 {
            Error::UnsupportedState
        } else {
            Error::InvalidResponse
        };
        assert!(matches!(result, Err(actual) if actual == expected));
    }
    assert_eq!(fixture.close().await.len(), 7);
    capture.assert_workflows(
        &[
            ("invalid_response", 0),
            ("invalid_response", 0),
            ("invalid_response", 0),
            ("invalid_response", 0),
            ("invalid_response", 0),
            ("invalid_response", 0),
            ("unsupported_state", 0),
        ],
        &[TOKEN, TRANSFER_ID, "private"],
    );
}

#[tokio::test]
async fn progress_is_a_finite_unit_fraction_and_not_a_percentage() {
    let mut bodies = [-0.1, 1.1, 42.0]
        .map(|fraction| serde_json::to_vec(&transfer("running", fraction)).unwrap())
        .to_vec();
    bodies.push(format!(r#"{{"status":"success","transfers":[{{"id":"{TRANSFER_ID}","created_at":{CREATED_AT},"status":"running","progress":1e999}}]}}"#).into_bytes());
    bodies.push(serde_json::to_vec(&transfer("running", 0.42)).unwrap());
    bodies.push(serde_json::to_vec(&transfer("seeding", 0.5)).unwrap());
    let fixture = Fixture::start(bodies).await;
    let client = client(&fixture);
    for _ in 0..4 {
        assert!(matches!(
            client.observe(&binding()).await,
            Err(Error::InvalidResponse)
        ));
    }
    let observation = client.observe(&binding()).await.unwrap();
    assert_eq!(observation.progress.fraction(), 0.42);
    let seeding = client.observe(&binding()).await.unwrap();
    assert_eq!(
        seeding.phase,
        hito_debrid_premiumize::TransferPhase::ProviderSeeding
    );
    assert_eq!(seeding.progress.fraction(), 0.5);
    assert_eq!(fixture.close().await.len(), 6);
}
