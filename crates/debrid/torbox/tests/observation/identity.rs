use super::*;

#[tokio::test]
async fn every_bound_identity_field_and_ambiguous_selected_file_fail_closed() {
    let mut replies = Vec::new();
    for mutation in 0..8 {
        let mut response: serde_json::Value =
            serde_json::from_slice(&observation(true, true, "album/owned.wav")).unwrap();
        match mutation {
            0 => response["data"]["id"] = json!(7124),
            1 => response["data"]["hash"] = json!("1123456789abcdef0123456789abcdef01234567"),
            2 => response["data"]["size"] = json!(1201),
            3 => response["data"]["files"][1]["id"] = json!(9),
            4 => response["data"]["files"][1]["name"] = json!("album/owned-other.wav"),
            5 => response["data"]["files"][1]["size"] = json!(401),
            6 => {
                response["data"]["files"][0] = response["data"]["files"][1].clone();
            }
            7 => response["data"]["files"] = json!([]),
            _ => unreachable!(),
        }
        replies.push(serde_json::to_vec(&response).unwrap());
    }
    let fixture = Fixture::start(replies).await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let binding = configured_binding();
    let persisted = serde_json::to_vec(&binding).unwrap();
    let capture = Capture::default();
    for _ in 0..8 {
        assert!(matches!(
            client
                .observe(&binding)
                .with_subscriber(capture.dispatch())
                .await,
            Err(Error::SourceChanged)
        ));
        assert!(serde_json::to_vec(&binding).unwrap() == persisted);
    }
    assert_eq!(fixture.close().await.len(), 8);
    capture.assert_observations(
        &[("source_changed", 0); 8],
        &[
            TOKEN,
            &binding.info_hash,
            &binding.file_name,
            "album/owned-other.wav",
        ],
    );
}
