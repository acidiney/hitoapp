use hito_plugin_sdk::Manifest;

#[test]
fn manifest_retains_optional_pkce_authorization_without_changing_legacy_wire_format() {
    let legacy = serde_json::json!({
        "id": "org.hito.spotify",
        "name": "Spotify playlists",
        "protocol_version": 1,
        "capabilities": ["playlists"]
    });
    let manifest: Manifest = serde_json::from_value(legacy.clone()).unwrap();
    manifest.validate().unwrap();
    let serialized = serde_json::to_value(manifest).unwrap();
    assert_eq!(serialized, legacy);
    assert!(serialized.get("authorization").is_none());

    let mut authorized = legacy;
    authorized["authorization"] = serde_json::json!({
        "kind": "oauth2_pkce",
        "client_id": "owned-test-client",
        "authorization_endpoint": "https://accounts.spotify.com/authorize",
        "scopes": ["playlist-read-private"]
    });
    let manifest: Manifest = serde_json::from_value(authorized.clone())
        .expect("A plugin manifest must accept a PKCE authorization declaration");
    manifest.validate().unwrap();
    assert_eq!(serde_json::to_value(manifest).unwrap(), authorized);
}

#[test]
fn manifest_authorization_accepts_secure_endpoints_and_rejects_unsafe_endpoints_safely() {
    for endpoint in [
        "https://accounts.spotify.com/authorize",
        "http://127.0.0.1:49152/authorize",
        "http://[::1]:49152/authorize",
    ] {
        let mut value = authorized_manifest();
        value["authorization"]["authorization_endpoint"] = endpoint.into();
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        manifest.validate().unwrap();
    }

    for endpoint in [
        "http://accounts.spotify.com/authorize",
        "http://localhost:49152/authorize",
        "http://192.0.2.1/authorize",
        "http://[2001:db8::1]/authorize",
        "ftp://127.0.0.1/authorize",
        "https://owned-user:owned-secret@example.com/authorize",
        "https://owned-user@example.com/authorize",
        "https://accounts.spotify.com/authorize?owned-query=private",
        "https://accounts.spotify.com/authorize#owned-fragment",
        "https://accounts.spotify.com/authorize path",
        "https://accounts.spotify.com/authorize\n",
        "https://accounts.spotify.com/author\tize",
        "/authorize",
        "",
        &format!("https://accounts.spotify.com/{}", "a".repeat(2048)),
    ] {
        let mut value = authorized_manifest();
        value["authorization"]["authorization_endpoint"] = endpoint.into();
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        assert_eq!(
            manifest.validate().unwrap_err(),
            "OAuth authorization endpoints must use HTTPS or a loopback IP over HTTP without credentials, query, or fragment"
        );
    }
}

#[test]
fn manifest_authorization_bounds_public_client_ids_and_rejects_invalid_ids_safely() {
    for client_id in [
        "owned-test-client".to_owned(),
        "a".repeat(256),
        "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~".to_owned(),
    ] {
        let mut value = authorized_manifest();
        value["authorization"]["client_id"] = client_id.into();
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        manifest.validate().unwrap();
    }

    for client_id in [
        "".to_owned(),
        "a".repeat(257),
        "owned client".to_owned(),
        "owned\nclient".to_owned(),
        "owned\tclient".to_owned(),
        "owned\0client".to_owned(),
        "owned\u{7f}client".to_owned(),
        "owned-café".to_owned(),
    ] {
        let mut value = authorized_manifest();
        value["authorization"]["client_id"] = client_id.into();
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        assert_eq!(
            manifest.validate().unwrap_err(),
            "OAuth client IDs must contain 1 to 256 visible ASCII bytes"
        );
    }
}

#[test]
fn manifest_authorization_bounds_scope_tokens_and_rejects_invalid_scopes_safely() {
    for scopes in [
        Vec::<String>::new(),
        vec!["playlist-read-private".to_owned()],
        vec!["!#$%&'()*+,-./:;<=>?@[]^_`{|}~".to_owned()],
        vec!["a".repeat(128)],
        (0..32)
            .map(|index| format!("owned-scope-{index}"))
            .collect(),
    ] {
        let mut value = authorized_manifest();
        value["authorization"]["scopes"] = serde_json::json!(scopes);
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        manifest.validate().unwrap();
    }

    for scopes in [
        vec!["".to_owned()],
        vec!["a".repeat(129)],
        (0..33)
            .map(|index| format!("owned-scope-{index}"))
            .collect(),
        vec!["playlist read".to_owned()],
        vec!["playlist\"read".to_owned()],
        vec!["playlist\\read".to_owned()],
        vec!["playlist\nread".to_owned()],
        vec!["playlist\tread".to_owned()],
        vec!["playlist\0read".to_owned()],
        vec!["playlist\u{7f}read".to_owned()],
        vec!["playlist-café".to_owned()],
    ] {
        let mut value = authorized_manifest();
        value["authorization"]["scopes"] = serde_json::json!(scopes);
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        assert_eq!(
            manifest.validate().unwrap_err(),
            "Declare at most 32 OAuth scope tokens of 1 to 128 bytes"
        );
    }
}

#[test]
fn manifest_authorization_rejects_unknown_incomplete_and_malformed_declarations() {
    let mut unknown_kind = authorized_manifest();
    unknown_kind["authorization"]["kind"] = "client_credentials".into();
    assert!(serde_json::from_value::<Manifest>(unknown_kind).is_err());

    for field in ["client_secret", "token_endpoint"] {
        let mut value = authorized_manifest();
        value["authorization"][field] = "owned-rejected-value".into();
        assert!(serde_json::from_value::<Manifest>(value).is_err());
    }

    for field in ["kind", "client_id", "authorization_endpoint", "scopes"] {
        let mut value = authorized_manifest();
        value["authorization"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<Manifest>(value).is_err());
    }

    for declaration in [
        serde_json::json!("oauth2_pkce"),
        serde_json::json!([]),
        serde_json::json!(42),
        serde_json::json!(true),
    ] {
        let mut value = authorized_manifest();
        value["authorization"] = declaration;
        assert!(serde_json::from_value::<Manifest>(value).is_err());
    }

    for (field, malformed) in [
        ("kind", serde_json::json!(42)),
        ("client_id", serde_json::json!(42)),
        ("authorization_endpoint", serde_json::json!([])),
        ("scopes", serde_json::json!("playlist-read-private")),
    ] {
        let mut value = authorized_manifest();
        value["authorization"][field] = malformed;
        assert!(serde_json::from_value::<Manifest>(value).is_err());
    }
}

fn authorized_manifest() -> serde_json::Value {
    serde_json::json!({
        "id": "org.hito.spotify",
        "name": "Spotify playlists",
        "protocol_version": 1,
        "capabilities": ["playlists"],
        "authorization": {
            "kind": "oauth2_pkce",
            "client_id": "owned-test-client",
            "authorization_endpoint": "https://accounts.spotify.com/authorize",
            "scopes": ["playlist-read-private"]
        }
    })
}
