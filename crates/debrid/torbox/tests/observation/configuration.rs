use super::*;

#[tokio::test]
async fn invalid_bindings_are_rejected_before_any_provider_request() {
    let mut invalid = Vec::new();
    for mutation in 0..8 {
        let mut binding = configured_binding();
        match mutation {
            0 => binding.torrent_id = 0,
            1 => binding.info_hash = "not-a-canonical-bittorrent-v1-hash".into(),
            2 => binding.torrent_bytes = 0,
            3 => binding.file_bytes = 0,
            4 => binding.file_bytes = binding.torrent_bytes + 1,
            5 => binding.file_name.clear(),
            6 => binding.file_name = "private\0name".into(),
            7 => binding.file_name = "a".repeat(4097),
            _ => unreachable!(),
        }
        invalid.push(binding);
    }
    let fixture = Fixture::start(vec![
        observation(true, true, "album/owned.wav");
        invalid.len()
    ])
    .await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let capture = Capture::default();
    for binding in invalid {
        assert!(
            matches!(
                client
                    .observe(&binding)
                    .with_subscriber(capture.dispatch())
                    .await,
                Err(Error::InvalidBinding)
            ),
            "invalid trusted configuration must not reach the provider"
        );
    }
    assert!(fixture.close().await.is_empty());
    capture.assert_observations(
        &[("invalid_binding", 0); 8],
        &[TOKEN, "private", "album/owned.wav"],
    );
}

#[tokio::test]
async fn credentials_and_test_tls_authority_are_checked_without_dispatch() {
    for token in [
        String::new(),
        "has space".into(),
        "line\nbreak".into(),
        "unicode-ç".into(),
        "a".repeat(4097),
    ] {
        assert!(matches!(
            TorBoxClient::new(token),
            Err(Error::InvalidCredential)
        ));
    }
    let fixture = Fixture::start(Vec::new()).await;
    for address in ["192.0.2.1:443", "127.0.0.1:0", "[2001:db8::1]:443"] {
        assert!(matches!(
            TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), address.parse().unwrap()),
            Err(Error::Configuration)
        ));
    }
    for ca in [Vec::new(), vec![0; 16_385], b"invalid owned CA".to_vec()] {
        assert!(matches!(
            TorBoxClient::with_test_tls(TOKEN.into(), &ca, fixture.address()),
            Err(Error::Configuration)
        ));
    }
    assert!(fixture.close().await.is_empty());
}
