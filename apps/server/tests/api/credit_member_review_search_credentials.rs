use crate::{
    account::exchange,
    artist_review_fixture::origin_counts,
    credit_member_review_credentials as fixture,
    credit_member_review_credentials_provider as provider,
    credit_member_review_owned::{Library, Response},
};
use axum::http::{Method, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use hito_testkit::fake_plugin::Reply;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};

#[tokio::test]
async fn member_search_credentials_follow_current_account_generation_and_validated_success() {
    let mut library = Library::retain();
    let outcome = AssertUnwindSafe(Box::pin(async {
        let context = hang_guard(
            "owned member credential HTTP setup",
            Box::pin(async {
                library.open().await;
                Box::pin(fixture::prepare(&library)).await
            }),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(exercise(&mut library, &context)),
        )
        .await
        .expect("The member credential lifecycle must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    library.close().await;
    let (proof, probes) = match outcome {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        proof.0, proof.1,
        "A new generation must not inherit an older credential diagnosis"
    );
    for probe in probes {
        fixture::private(&probe.response.1);
        match probe.expected.wire {
            Wire::Credentials(stamp) => {
                assert_eq!(
                    probe.response.0,
                    StatusCode::CONFLICT,
                    "{}",
                    probe.expected.name
                );
                let error = &probe.response.2;
                assert_eq!(error["code"], "plugin_credentials_required");
                assert_eq!(error["plugin_connection"], stamp);
                assert_eq!(error.as_object().unwrap().len(), 3);
                assert!(
                    error["error"]
                        .as_str()
                        .is_some_and(|error| !error.is_empty() && error.len() <= 240)
                );
                let encoded = error.to_string();
                assert!(
                    !encoded.contains(provider::OWNER_TOKEN)
                        && !encoded.contains(provider::PEER_TOKEN)
                );
            }
            Wire::State(state) => {
                assert_eq!(probe.response.0, StatusCode::OK, "{}", probe.expected.name);
                assert_eq!(probe.response.2, json!({"status": state}));
            }
            Wire::Available(context) => {
                assert_eq!(probe.response.0, StatusCode::OK, "{}", probe.expected.name);
                assert_eq!(
                    probe.response.2,
                    json!({"status": "available", "context": context, "page": provider::page()})
                );
            }
            Wire::Stale => {
                assert_eq!(
                    probe.response.0,
                    StatusCode::CONFLICT,
                    "{}",
                    probe.expected.name
                );
                assert!(probe.response.2["error"].is_string());
                assert!(probe.response.2.get("plugin_connection").is_none());
                assert!(probe.response.2.get("code").is_none());
            }
        }
        assert_eq!(
            probe.settings, probe.expected.settings,
            "{}",
            probe.expected.name
        );
        if let Some(during) = probe.during {
            assert_eq!(
                during, probe.expected.settings,
                "Descriptor success cannot clear credentials while search is held"
            );
        }
        let calls: Vec<_> = probe
            .expected
            .operations
            .iter()
            .map(|operation| {
                let method = if *operation == provider::SEARCH {
                    Method::POST
                } else {
                    Method::GET
                };
                let body = if *operation == provider::SEARCH {
                    provider::query()
                } else {
                    Value::Null
                };
                (method, operation.to_string(), body)
            })
            .collect();
        assert_eq!(probe.calls, calls, "{}", probe.expected.name);
    }
}

enum Wire {
    Credentials(Value),
    State(&'static str),
    Available(Value),
    Stale,
}

struct Expected {
    name: &'static str,
    wire: Wire,
    settings: Value,
    operations: Vec<&'static str>,
}

struct Probe {
    response: Response,
    settings: Value,
    calls: Vec<(Method, String, Value)>,
    during: Option<Value>,
    expected: Expected,
}

async fn exercise(
    library: &mut Library,
    context: &fixture::Context,
) -> ((Value, Value), Vec<Probe>) {
    let app = library.app();
    let baseline = context.owner.get(app, "/api/v1/plugins").await;
    let original_row = fixture::installation(&baseline, &context.installation["id"]);
    assert_eq!(
        original_row["health"],
        json!({"state": "ok", "reason": null, "since": null})
    );
    let peer = context.peer.get(app, "/api/v1/plugins").await;
    assert_eq!(
        fixture::installation(&peer, &context.peer_installation["id"])["health"],
        json!({"state": "ok", "reason": null, "since": null})
    );
    let stamp = |revision: &Value| json!({"installation_id": context.installation["id"], "installation_revision": revision});
    let mut probes = Vec::new();
    provider::reject(library.plugin(), provider::DESCRIPTOR);
    probes.push(
        probe(
            library,
            context,
            &context.context,
            Expected {
                name: "current descriptor rejection",
                wire: Wire::Credentials(stamp(&context.installation["revision"])),
                settings: rejected(&baseline, &context.installation["id"]),
                operations: vec![provider::DESCRIPTOR],
            },
        )
        .await,
    );
    let refreshed = fixture::accepted(
        library.app(),
        &context.owner,
        "POST",
        &format!(
            "/api/v1/plugins/{}/refresh",
            context.installation["id"].as_str().unwrap()
        ),
        json!({"revision": context.installation["revision"]}),
    )
    .await;
    assert_eq!(refreshed["kind"], "updated");
    assert_eq!(
        refreshed["installation"]["revision"].as_i64().unwrap(),
        context.installation["revision"].as_i64().unwrap() + 1
    );
    let mut expected_new = baseline.clone();
    let current = expected_new["installations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == context.installation["id"])
        .unwrap();
    let health = current.get("health").cloned();
    *current = refreshed["installation"].clone();
    if let Some(health) = health {
        current["health"] = health;
    }
    let new_baseline = context.owner.get(library.app(), "/api/v1/plugins").await;
    let proof = (new_baseline.clone(), expected_new);
    probes.push(
        probe(
            library,
            context,
            &context.context,
            Expected {
                name: "obsolete context after refresh",
                wire: Wire::Stale,
                settings: new_baseline.clone(),
                operations: vec![],
            },
        )
        .await,
    );
    let fresh = context.fresh_context(library).await;
    let mut expected_context = context.context.clone();
    expected_context["connection"]["installation_revision"] =
        refreshed["installation"]["revision"].clone();
    assert_eq!(fresh, expected_context);
    let required = rejected(&new_baseline, &context.installation["id"]);
    let current_stamp = stamp(&refreshed["installation"]["revision"]);
    library
        .plugin()
        .reply(provider::DESCRIPTOR, provider::descriptor());
    provider::reject(library.plugin(), provider::SEARCH);
    probes.push(
        probe(
            library,
            context,
            &fresh,
            Expected {
                name: "current search rejection",
                wire: Wire::Credentials(current_stamp.clone()),
                settings: required.clone(),
                operations: vec![provider::DESCRIPTOR, provider::SEARCH],
            },
        )
        .await,
    );
    library
        .plugin()
        .reply(provider::DESCRIPTOR, Reply::Status(StatusCode::NOT_FOUND));
    probes.push(
        probe(
            library,
            context,
            &fresh,
            Expected {
                name: "missing descriptor",
                wire: Wire::State("unsupported"),
                settings: required.clone(),
                operations: vec![provider::DESCRIPTOR],
            },
        )
        .await,
    );
    library
        .plugin()
        .reply(provider::DESCRIPTOR, provider::unsupported());
    probes.push(
        probe(
            library,
            context,
            &fresh,
            Expected {
                name: "unsupported descriptor200",
                wire: Wire::State("unsupported"),
                settings: required.clone(),
                operations: vec![provider::DESCRIPTOR],
            },
        )
        .await,
    );
    library
        .plugin()
        .reply(provider::DESCRIPTOR, provider::descriptor());
    let before = library.plugin().requests().len();
    let origins = origin_counts(library.origin()).await;
    library.hold_search(
        context.owner.headers(),
        context.path(),
        json!({"context": fresh, "search": provider::query()}),
    );
    assert!(
        library.search_entered().await,
        "The owned protected search must reach its hold"
    );
    assert!(library.search_pending());
    let during = context.owner.get(library.app(), "/api/v1/plugins").await;
    let held_response = library.complete_search().await;
    let after = context.owner.get(library.app(), "/api/v1/plugins").await;
    context.assert_private(library).await;
    assert_eq!(origin_counts(library.origin()).await, origins);
    probes.push(Probe {
        response: held_response,
        settings: after,
        calls: calls(library, before),
        during: Some(during),
        expected: Expected {
            name: "descriptor success before protected failure",
            wire: Wire::Credentials(current_stamp),
            settings: required.clone(),
            operations: vec![provider::DESCRIPTOR, provider::SEARCH],
        },
    });
    provider::malformed_search(library.plugin());
    probes.push(
        probe(
            library,
            context,
            &fresh,
            Expected {
                name: "malformed protected page",
                wire: Wire::State("unavailable"),
                settings: required,
                operations: vec![provider::DESCRIPTOR, provider::SEARCH],
            },
        )
        .await,
    );
    provider::valid_search(library.plugin());
    probes.push(
        probe(
            library,
            context,
            &fresh,
            Expected {
                name: "current validated recovery",
                wire: Wire::Available(fresh.clone()),
                settings: new_baseline,
                operations: vec![provider::DESCRIPTOR, provider::SEARCH],
            },
        )
        .await,
    );
    assert_eq!(
        context.peer.get(library.app(), "/api/v1/plugins").await,
        peer
    );
    assert_eq!(library.plugin().unexpected(), 0);
    (proof, probes)
}

async fn probe(
    library: &Library,
    context: &fixture::Context,
    captured: &Value,
    expected: Expected,
) -> Probe {
    let before = library.plugin().requests().len();
    let origins = origin_counts(library.origin()).await;
    let response = exchange(
        library.app(),
        "POST",
        &context.path(),
        json!({"context": captured, "search": provider::query()}),
        context.owner.headers(),
    )
    .await;
    let settings = context.owner.get(library.app(), "/api/v1/plugins").await;
    context.assert_private(library).await;
    assert_eq!(origin_counts(library.origin()).await, origins);
    Probe {
        response,
        settings,
        calls: calls(library, before),
        during: None,
        expected,
    }
}

fn calls(library: &Library, before: usize) -> Vec<(Method, String, Value)> {
    library.plugin().requests()[before..]
        .iter()
        .map(|request| {
            assert!(provider::owner_request(request));
            (
                request.method.clone(),
                request.operation.clone(),
                request.json(),
            )
        })
        .collect()
}

fn rejected(baseline: &Value, id: &Value) -> Value {
    let mut required = baseline.clone();
    let row = required["installations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == *id)
        .unwrap();
    row["health"] = json!({"state": "credentials_required", "reason": null, "since": null});
    required
}
