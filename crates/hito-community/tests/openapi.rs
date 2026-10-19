use std::{collections::BTreeSet, future::Future, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use hito_community::{Config, Service, router};
use serde_json::Value;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";

struct OperationContract {
    path: &'static str,
    authority: Option<&'static str>,
    request_fields: &'static [&'static str],
    success_status: &'static str,
    response_fields: &'static [&'static str],
}

#[tokio::test]
async fn the_public_contract_describes_actual_routes_authority_and_strict_bounded_wire_types() {
    assert_eq!(
        ADMIN_TOKEN.len(),
        64,
        "Administrator fixture must be 64 hex characters"
    );
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("openapi-private-state-sentinel");
    let service = bounded(Service::open(Config {
        state_directory: directory.clone(),
        admin_token: ADMIN_TOKEN.to_owned(),
    }))
    .await
    .unwrap();
    let app = router(service.clone());
    let response = bounded(
        app.clone().oneshot(
            Request::builder()
                .uri("/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        ),
    )
    .await
    .unwrap();
    let status = response.status();
    let bytes = bounded(to_bytes(response.into_body(), 256 * 1024))
        .await
        .unwrap();
    bounded(service.close()).await;
    assert_eq!(status, StatusCode::OK);
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    if let Some(path) = std::env::var_os("HITO_COMMUNITY_TEST_CONTRACT_CAPTURE") {
        std::fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    }
    assert!(document["openapi"].as_str().unwrap().starts_with("3."));
    assert!(!document["info"]["title"].as_str().unwrap().is_empty());
    assert!(!document["info"]["version"].as_str().unwrap().is_empty());
    let paths = document["paths"].as_object().unwrap();
    assert_eq!(paths.len(), 5);
    assert_public(&document, &paths["/v1/openapi.json"]["get"]);
    assert!(paths["/v1/openapi.json"]["get"]["responses"]["200"].is_object());
    let revoke = &paths["/v1/admin/reporters/{reporter_id}"]["delete"];
    assert!(revoke.is_object());
    assert_eq!(
        effective_security(&document, revoke),
        Some(&serde_json::json!([{"administratorBearer": []}]))
    );
    assert!(revoke.get("requestBody").is_none());
    let parameter = &revoke["parameters"][0];
    assert_eq!(parameter["name"], "reporter_id");
    assert_eq!(parameter["in"], "path");
    assert_eq!(parameter["required"], true);
    assert_eq!(resolve(&document, &parameter["schema"])["format"], "uuid");
    assert!(revoke["responses"]["204"].is_object());
    assert!(revoke["responses"]["204"].get("content").is_none());
    for status in ["400", "401", "503"] {
        let rejection = component(
            &document,
            &revoke["responses"][status]["content"]["application/json"]["schema"],
        );
        assert!(rejection["properties"]["error"].is_object());
    }

    let operations = [
        OperationContract {
            path: "/v1/admin/reporters",
            authority: Some("administratorBearer"),
            request_fields: &[],
            success_status: "201",
            response_fields: &["reporter_id", "token"],
        },
        OperationContract {
            path: "/v1/outcomes",
            authority: Some("reporterBearer"),
            request_fields: &["submission_id", "source", "outcome"],
            success_status: "201",
            response_fields: &["receipt_id", "submission_id", "status"],
        },
        OperationContract {
            path: "/v1/rankings",
            authority: None,
            request_fields: &["candidates"],
            success_status: "200",
            response_fields: &["ranked"],
        },
    ];
    for contract in operations {
        let operation = &paths[contract.path]["post"];
        assert!(operation.is_object());
        if let Some(authority) = contract.authority {
            let security = effective_security(&document, operation)
                .unwrap()
                .as_array()
                .unwrap();
            assert!(!security.is_empty());
            for requirement in security {
                let requirement = requirement.as_object().unwrap();
                assert_eq!(requirement.len(), 1);
                assert_eq!(requirement[authority].as_array().unwrap().len(), 0);
            }
            let scheme = &document["components"]["securitySchemes"][authority];
            assert_eq!(scheme["type"], "http");
            assert_eq!(scheme["scheme"], "bearer");
            assert!(operation["responses"]["401"].is_object());
        } else {
            assert_public(&document, operation);
        }
        let request = component(
            &document,
            &operation["requestBody"]["content"]["application/json"]["schema"],
        );
        assert_eq!(request["additionalProperties"], false);
        let expected = contract
            .request_fields
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            request["properties"]
                .as_object()
                .map(|properties| properties
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>())
                .unwrap_or_default(),
            expected
        );
        assert!(operation["requestBody"]["required"].as_bool().unwrap());
        let limit = format!(
            "{} {}",
            description(operation),
            description(&operation["requestBody"])
        );
        let limit = limit.to_lowercase().replace([',', '_', ' '], "");
        assert!(limit.contains("16384") || limit.contains("16kib"));
        assert!(operation["responses"]["413"].is_object());
        let response = component(
            &document,
            &operation["responses"][contract.success_status]["content"]["application/json"]["schema"],
        );
        for field in contract.response_fields {
            assert!(response["properties"][field].is_object());
        }
    }

    let outcome = component(
        &document,
        &paths["/v1/outcomes"]["post"]["requestBody"]["content"]["application/json"]["schema"],
    );
    let submission_id = &outcome["properties"]["submission_id"];
    assert_eq!(resolve(&document, submission_id)["format"], "uuid");
    let version = description(submission_id).to_lowercase();
    assert!(
        version.contains("version 4") || version.contains("version-4") || version.contains("v4")
    );
    let kind = resolve(&document, &outcome["properties"]["outcome"]);
    assert_eq!(kind["enum"], serde_json::json!(["successful_playback"]));
    let source = resolve(&document, &outcome["properties"]["source"]);
    assert_eq!(source["additionalProperties"], false);
    let providers = resolve(&document, &source["properties"]["provider"]);
    assert_eq!(
        providers["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|provider| provider.as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["jamendo", "youtube"])
    );
    let recording_id = &source["properties"]["recording_id"];
    assert_eq!(recording_id["minLength"], 1);
    assert_eq!(recording_id["maxLength"], 20);
    let ids = description(recording_id).to_lowercase();
    assert!(ids.contains("youtube") && ids.contains("11"));
    assert!(ids.contains("jamendo") && ids.contains("leading zero"));
    assert!(
        ids.contains("u64")
            || ids.contains("18446744073709551615")
            || ids.contains("64-bit")
            || ids.contains("64 bit")
    );
    assert!(paths["/v1/outcomes"]["post"]["responses"]["409"].is_object());
    let limited = &paths["/v1/outcomes"]["post"]["responses"]["429"];
    component(&document, &limited["content"]["application/json"]["schema"]);
    let retry = &limited["headers"]["Retry-After"];
    assert_eq!(retry["schema"]["type"], "string");
    assert!(description(retry).contains("60") && description(retry).contains("300"));
    component(
        &document,
        &paths["/v1/outcomes"]["post"]["responses"]["200"]["content"]["application/json"]["schema"],
    );

    let ranks = component(
        &document,
        &paths["/v1/rankings"]["post"]["requestBody"]["content"]["application/json"]["schema"],
    );
    let candidates = &ranks["properties"]["candidates"];
    assert_eq!(candidates["minItems"], 1);
    assert_eq!(candidates["maxItems"], 32);
    assert_eq!(candidates["uniqueItems"], true);
    assert_eq!(resolve(&document, &candidates["items"]), source);
    let serialized = document.to_string();
    for private in [
        ADMIN_TOKEN,
        "openapi-private-state-sentinel",
        directory.to_str().unwrap(),
    ] {
        assert!(!serialized.contains(private));
    }
}

fn effective_security<'a>(document: &'a Value, operation: &'a Value) -> Option<&'a Value> {
    operation
        .get("security")
        .or_else(|| document.get("security"))
}

fn assert_public(document: &Value, operation: &Value) {
    assert!(operation.is_object());
    if let Some(security) = effective_security(document, operation) {
        let requirements = security.as_array().unwrap();
        assert!(
            requirements.is_empty()
                || requirements.iter().any(|requirement| requirement
                    .as_object()
                    .is_some_and(|requirement| requirement.is_empty()))
        );
    }
}

fn component<'a>(document: &'a Value, schema: &'a Value) -> &'a Value {
    let reference = schema["$ref"].as_str().unwrap();
    assert!(reference.starts_with("#/components/schemas/"));
    resolve(document, schema)
}

fn resolve<'a>(document: &'a Value, mut schema: &'a Value) -> &'a Value {
    for _ in 0..16 {
        let Some(reference) = schema.get("$ref") else {
            return schema;
        };
        schema = document
            .pointer(reference.as_str().unwrap().strip_prefix('#').unwrap())
            .unwrap();
    }
    panic!("OpenAPI schema references must terminate");
}

fn description(value: &Value) -> &str {
    value["description"].as_str().unwrap_or_default()
}

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("Public contract workflow exceeded its deadline")
}
