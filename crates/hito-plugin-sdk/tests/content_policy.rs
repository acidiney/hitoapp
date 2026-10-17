use hito_plugin_sdk::{ContentPolicy, Manifest, SourcePersistence};
use serde_json::json;

const LEGACY: &[u8] = br#"{"id":"org.hito.policy-fixture","name":"Policy fixture","protocol_version":1,"capabilities":["search","sources"]}"#;

#[test]
fn an_absent_content_policy_preserves_exact_legacy_manifest_fingerprint_bytes() {
    let manifest: Manifest = serde_json::from_slice(LEGACY).unwrap();
    manifest.validate().unwrap();
    assert!(manifest.authorization.is_none());
    assert!(manifest.content_policy.is_none());
    assert_eq!(serde_json::to_vec(&manifest).unwrap(), LEGACY);
}

#[test]
fn declared_source_persistence_round_trips_through_the_public_typed_manifest() {
    for (source_persistence, spelling) in [
        (SourcePersistence::ExplicitDownload, "explicit_download"),
        (SourcePersistence::PlaybackOnly, "playback_only"),
    ] {
        let wire = format!(
            r#"{{"id":"org.hito.policy-fixture","name":"Policy fixture","protocol_version":1,"capabilities":["search","sources"],"content_policy":{{"source_persistence":"{spelling}"}}}}"#
        );
        let manifest: Manifest = serde_json::from_str(&wire).unwrap();
        manifest.validate().unwrap();
        assert_eq!(
            manifest.content_policy,
            Some(ContentPolicy { source_persistence })
        );
        assert_eq!(serde_json::to_vec(&manifest).unwrap(), wire.as_bytes());
    }
}

#[test]
fn malformed_content_policy_declarations_fail_during_manifest_deserialization() {
    for content_policy in [
        json!({"source_persistence": "playback_only", "unexpected": true}),
        json!({"source_persistence": "unknown_policy"}),
        json!({}),
    ] {
        let mut manifest: serde_json::Value = serde_json::from_slice(LEGACY).unwrap();
        manifest["content_policy"] = content_policy;
        let result: Result<Manifest, serde_json::Error> = serde_json::from_value(manifest);
        assert!(result.is_err());
    }
}
