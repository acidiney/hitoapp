use super::*;

#[tokio::test]
async fn provider_error_cannot_be_promoted_by_success_flags_and_matching_file_metadata() {
    let mut response: serde_json::Value =
        serde_json::from_slice(&observation(true, true, "album/owned.wav")).unwrap();
    response["error"] = json!({ "code": "owned-private-provider-rejection" });
    let fixture = Fixture::start(vec![serde_json::to_vec(&response).unwrap()]).await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let capture = Capture::default();
    let result = client
        .observe(&configured_binding())
        .with_subscriber(capture.dispatch())
        .await;
    assert_eq!(fixture.close().await.len(), 1);
    assert!(
        matches!(result, Err(Error::ProviderRejected)),
        "an explicit provider error must fail closed"
    );
    capture.assert_observations(
        &[("provider_rejected", 0)],
        &[TOKEN, "owned-private-provider-rejection", "album/owned.wav"],
    );
}
