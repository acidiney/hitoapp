mod fixture;
mod release;
mod resumption;

#[test]
fn begin_request_has_one_strict_shared_wire_shape() {
    let request: hito_core::BeginCatalogSnapshot =
        serde_json::from_value(serde_json::json!({"library_id": "library"})).unwrap();
    assert_eq!(request.library_id, "library");
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        serde_json::json!({"library_id": "library"})
    );
    assert!(
        serde_json::from_value::<hito_core::BeginCatalogSnapshot>(
            serde_json::json!({"library_id": "library", "cursor": 7})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<hito_core::BeginCatalogSnapshot>(serde_json::json!({})).is_err()
    );
    let schema = <hito_core::BeginCatalogSnapshot as utoipa::PartialSchema>::schema();
    assert!(serde_json::to_value(schema).unwrap()["properties"]["library_id"].is_object());
}
