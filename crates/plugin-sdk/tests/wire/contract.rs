use hito_plugin_sdk::Manifest;

#[test]
fn manifest_negotiation_rejects_unknown_versions_and_capabilities() {
    let value = serde_json::json!({"id":"org.hito.folder","name":"Music folder","protocol_version":1,"capabilities":["search","sources","lyrics","backing_tracks"]});
    let manifest: Manifest = serde_json::from_value(value.clone()).unwrap();
    assert!(manifest.validate().is_ok());
    let mut unknown = value.clone();
    unknown["capabilities"] = serde_json::json!(["execute_code"]);
    assert!(serde_json::from_value::<Manifest>(unknown).is_err());
    let mut future = value;
    future["protocol_version"] = serde_json::json!(99);
    assert!(
        serde_json::from_value::<Manifest>(future)
            .unwrap()
            .validate()
            .is_err()
    );
}
