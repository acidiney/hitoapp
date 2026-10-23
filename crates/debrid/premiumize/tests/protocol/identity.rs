use super::*;

#[tokio::test]
async fn missing_duplicate_and_replaced_transfer_records_never_select_a_decoy() {
    let mut absent = transfer("finished", 1.0);
    absent["transfers"][0]["id"] = json!("owned-decoy-transfer");
    let mut duplicate = transfer("finished", 1.0);
    let selected = duplicate["transfers"][0].clone();
    duplicate["transfers"]
        .as_array_mut()
        .unwrap()
        .push(selected);
    let mut replaced = transfer("finished", 1.0);
    replaced["transfers"][0]["created_at"] = json!(CREATED_AT + 1);
    let fixture = Fixture::start(
        [absent, duplicate, replaced]
            .into_iter()
            .map(|value| serde_json::to_vec(&value).unwrap())
            .collect(),
    )
    .await;
    let client = client(&fixture);
    let capture = Capture::default();
    for expected in [
        Error::TransferNotFound,
        Error::TransferChanged,
        Error::TransferChanged,
    ] {
        let result = client
            .observe(&binding())
            .with_subscriber(capture.dispatch())
            .await;
        assert!(matches!(result, Err(actual) if actual == expected));
    }
    assert_eq!(fixture.close().await.len(), 3);
    capture.assert_workflows(
        &[
            ("transfer_not_found", 0),
            ("transfer_changed", 0),
            ("transfer_changed", 0),
        ],
        &[TOKEN, TRANSFER_ID, "owned-decoy-transfer"],
    );
}

#[tokio::test]
async fn malformed_binding_and_credentials_do_not_dispatch() {
    let fixture = Fixture::start(Vec::new()).await;
    let client = client(&fixture);
    let capture = Capture::default();
    for id in [
        String::new(),
        " \t".into(),
        "private\nreference".into(),
        "x".repeat(257),
    ] {
        let configured = TransferBinding {
            transfer_id: id,
            ..binding()
        };
        assert!(matches!(
            client
                .observe(&configured)
                .with_subscriber(capture.dispatch())
                .await,
            Err(Error::InvalidBinding)
        ));
    }
    for token in [
        String::new(),
        "has space".into(),
        "line\nbreak".into(),
        "unicode-ç".into(),
        "x".repeat(4097),
    ] {
        assert!(matches!(
            PremiumizeClient::new(token),
            Err(Error::InvalidCredential)
        ));
    }
    for address in ["192.0.2.1:443", "127.0.0.1:0", "[2001:db8::1]:443"] {
        assert!(matches!(
            PremiumizeClient::with_test_tls(
                TOKEN.into(),
                fixture.ca_der(),
                address.parse().unwrap()
            ),
            Err(Error::Configuration)
        ));
    }
    for ca in [Vec::new(), vec![0; 16_385], b"invalid owned CA".to_vec()] {
        assert!(matches!(
            PremiumizeClient::with_test_tls(TOKEN.into(), &ca, fixture.address()),
            Err(Error::Configuration)
        ));
    }
    assert!(fixture.close().await.is_empty());
    capture.assert_workflows(
        &[("invalid_binding", 0); 4],
        &[TOKEN, TRANSFER_ID, "private"],
    );
}
