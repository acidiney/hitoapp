use axum::http::{HeaderMap, HeaderName, HeaderValue};
use hito_plugin_sdk::{Capability, MANIFEST_PATH, Manifest, PROTOCOL_VERSION};
use hito_testkit::fake_plugin::{FakePlugin, Fault, Gate, Handled, Reply};
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

fn manifest() -> Manifest {
    Manifest {
        id: "org.hito.fake".into(),
        name: "Fake".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Search].into(),
        authorization: None,
        content_policy: None,
    }
}

async fn echo() -> FakePlugin {
    FakePlugin::builder(manifest())
        .post("v1/search", |request| {
            Reply::Json(json!({"seen": request.json()}))
        })
        .get("v1/ping", |_| Reply::Json(json!({"pong": true})))
        .start()
        .await
}

fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::new()
}

#[tokio::test]
async fn serves_the_manifest_under_its_prefix() {
    let mut plugin = echo().await;
    let response = client()
        .get(format!("{}{MANIFEST_PATH}", plugin.url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let served: Manifest = response.json().await.unwrap();
    assert_eq!(served.id, "org.hito.fake");
    assert!(served.capabilities.contains(&Capability::Search));
    assert!(plugin.finish().await);
}

#[tokio::test]
async fn dispatches_each_operation_to_its_handler() {
    let plugin = echo().await;
    let search: Value = client()
        .post(format!("{}v1/search", plugin.url))
        .json(&json!({"q": 1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(search, json!({"seen": {"q": 1}}));
    let ping: Value = client()
        .get(format!("{}v1/ping", plugin.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ping, json!({"pong": true}));
}

#[tokio::test]
async fn unknown_operations_are_not_found_and_counted() {
    let plugin = echo().await;
    let response = client()
        .post(format!("{}v1/other", plugin.url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let wrong_method = client()
        .get(format!("{}v1/search", plugin.url))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_method.status(), StatusCode::NOT_FOUND);
    assert_eq!(plugin.unmatched(), 2);
    assert!(
        plugin
            .requests_for("v1/search")
            .iter()
            .all(|r| r.handled == Handled::No)
    );
}

#[tokio::test]
async fn request_log_records_operation_headers_and_body() {
    let plugin = echo().await;
    client()
        .post(format!("{}v1/search?page=2", plugin.url))
        .header("x-hito-plugin-protocol", "1")
        .bearer_auth("secret-token")
        .json(&json!({"q": "a"}))
        .send()
        .await
        .unwrap();
    client()
        .get(format!("{}v1/ping", plugin.url))
        .send()
        .await
        .unwrap();
    let log = plugin.requests();
    assert_eq!(log.len(), 2);
    let search = &log[0];
    assert_eq!(search.operation, "v1/search");
    assert_eq!(search.method, "POST");
    assert_eq!(search.query.as_deref(), Some("page=2"));
    assert_eq!(search.json(), json!({"q": "a"}));
    assert!(search.has_authorization());
    assert_eq!(search.protocol().as_deref(), Some("1"));
    assert_eq!(search.handled, Handled::Yes);
    let ping = &log[1];
    assert!(!ping.has_authorization());
    assert_eq!(ping.protocol(), None);
    assert_eq!(plugin.requests_for("v1/ping").len(), 1);
    assert_eq!(plugin.unmatched(), 0);
}

#[tokio::test]
async fn bearer_requirement_rejects_missing_wrong_and_unversioned_requests() {
    let plugin = FakePlugin::builder(manifest())
        .get("v1/ping", |_| Reply::Json(json!({})))
        .require_bearer("good-token")
        .start()
        .await;
    let url = format!("{}v1/ping", plugin.url);
    let send = |token: Option<&str>, protocol: Option<&str>| {
        let mut request = client().get(&url);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(protocol) = protocol {
            request = request.header("x-hito-plugin-protocol", protocol);
        }
        request.send()
    };
    assert_eq!(
        send(None, Some("1")).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(Some("bad"), Some("1")).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(Some("good-token"), None).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(Some("good-token"), Some("1")).await.unwrap().status(),
        StatusCode::OK
    );
    let handled: Vec<Handled> = plugin.requests().iter().map(|r| r.handled).collect();
    assert_eq!(
        handled,
        [
            Handled::Rejected,
            Handled::Rejected,
            Handled::Rejected,
            Handled::Yes
        ]
    );
}

#[tokio::test]
async fn protocol_requirement_alone_rejects_a_missing_header() {
    let plugin = FakePlugin::builder(manifest())
        .get("v1/ping", |_| Reply::Json(json!({})))
        .require_protocol()
        .start()
        .await;
    let url = format!("{}v1/ping", plugin.url);
    assert_eq!(
        client().get(&url).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        client()
            .get(&url)
            .header("x-hito-plugin-protocol", "1")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn status_fault_replaces_the_reply_and_can_be_cleared() {
    let plugin = echo().await;
    let url = format!("{}v1/ping", plugin.url);
    plugin.fault("v1/ping", Fault::Status(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(
        client().get(&url).send().await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    plugin.clear_fault("v1/ping");
    assert_eq!(
        client().get(&url).send().await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(plugin.requests_for("v1/ping").len(), 2);
}

#[tokio::test]
async fn malformed_fault_returns_a_body_that_is_not_json() {
    let plugin = echo().await;
    plugin.fault("v1/ping", Fault::Malformed);
    let response = client()
        .get(format!("{}v1/ping", plugin.url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = response.text().await.unwrap();
    assert!(serde_json::from_str::<Value>(&text).is_err());
}

#[tokio::test]
async fn delay_fault_postpones_the_normal_reply() {
    let plugin = echo().await;
    plugin.fault("v1/ping", Fault::Delay(Duration::from_millis(150)));
    let started = std::time::Instant::now();
    let response = client()
        .get(format!("{}v1/ping", plugin.url))
        .send()
        .await
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(150));
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"pong": true})
    );
}

#[tokio::test]
async fn drop_connection_fault_breaks_the_response_body() {
    let plugin = echo().await;
    plugin.fault("v1/ping", Fault::DropConnection);
    let result = async {
        let response = client()
            .get(format!("{}v1/ping", plugin.url))
            .send()
            .await?;
        response.bytes().await
    }
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn hold_fault_parks_the_request_until_released() {
    let plugin = echo().await;
    let gate: Arc<Gate> = plugin.hold("v1/ping");
    let url = format!("{}v1/ping", plugin.url);
    let pending = tokio::spawn(async move { client().get(url).send().await.unwrap().status() });
    gate.entered().await;
    assert!(!pending.is_finished());
    gate.release();
    assert_eq!(pending.await.unwrap(), StatusCode::OK);
}

#[tokio::test]
async fn replies_can_be_replaced_while_running() {
    let plugin = echo().await;
    plugin.reply("v1/ping", Reply::Json(json!({"pong": false})));
    plugin.reply(
        MANIFEST_PATH,
        Reply::Json(json!({"id": "org.hito.changed"})),
    );
    let ping: Value = client()
        .get(format!("{}v1/ping", plugin.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ping, json!({"pong": false}));
    let served: Value = client()
        .get(format!("{}{MANIFEST_PATH}", plugin.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(served["id"], "org.hito.changed");
}

#[tokio::test]
async fn raw_replies_carry_status_headers_and_chunked_bodies() {
    let plugin = echo().await;
    plugin.reply(
        "v1/ping",
        Reply::Raw {
            status: StatusCode::CREATED,
            headers: HeaderMap::from_iter([(
                HeaderName::from_static("x-fixture"),
                HeaderValue::from_static("yes"),
            )]),
            body: b"abc".to_vec(),
            chunked: true,
        },
    );
    let response = client()
        .get(format!("{}v1/ping", plugin.url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()["x-fixture"], "yes");
    assert!(response.headers().get("content-length").is_none());
    assert_eq!(response.text().await.unwrap(), "abc");
}

#[tokio::test]
async fn hold_and_delay_keep_the_reply_chosen_when_the_request_arrived() {
    let plugin = echo().await;
    let gate = plugin.hold("v1/ping");
    let url = format!("{}v1/ping", plugin.url);
    let pending = tokio::spawn(async move {
        client()
            .get(url)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()
    });
    gate.entered().await;
    plugin.reply("v1/ping", Reply::Json(json!({"pong": "later"})));
    gate.release();
    assert_eq!(pending.await.unwrap(), json!({"pong": true}));
}

#[tokio::test]
async fn waiting_for_requests_returns_once_the_log_has_that_many() {
    let plugin = echo().await;
    let url = format!("{}v1/ping", plugin.url);
    let sender = tokio::spawn(async move {
        for _ in 0..2 {
            client().get(&url).send().await.unwrap();
        }
    });
    tokio::time::timeout(Duration::from_secs(3), plugin.wait_for_requests(2))
        .await
        .expect("both requests must be logged");
    sender.await.unwrap();
    assert_eq!(plugin.requests().len(), 2);
}

#[tokio::test]
async fn bearer_requirement_can_change_while_running() {
    let plugin = echo().await;
    let url = format!("{}v1/ping", plugin.url);
    let status = |token: &'static str| {
        let request = client()
            .get(&url)
            .header("x-hito-plugin-protocol", "1")
            .bearer_auth(token);
        async move { request.send().await.unwrap().status() }
    };
    assert_eq!(status("any").await, StatusCode::OK);
    plugin.require_bearer("first");
    assert_eq!(status("any").await, StatusCode::UNAUTHORIZED);
    assert_eq!(status("first").await, StatusCode::OK);
    plugin.require_bearer("second");
    assert_eq!(status("first").await, StatusCode::UNAUTHORIZED);
    assert_eq!(status("second").await, StatusCode::OK);
}

#[tokio::test]
async fn held_gate_reports_when_the_parked_request_is_retired() {
    let plugin = echo().await;
    let url = format!("{}v1/ping", plugin.url);

    let gate = plugin.hold("v1/ping");
    let released = tokio::spawn({
        let url = url.clone();
        async move { client().get(url).send().await.unwrap().status() }
    });
    gate.entered().await;
    gate.release();
    assert_eq!(released.await.unwrap(), StatusCode::OK);
    tokio::time::timeout(Duration::from_secs(3), gate.retired())
        .await
        .expect("a released request must retire");

    let gate = plugin.hold("v1/ping");
    let abandoned = tokio::spawn(async move { client().get(url).send().await });
    gate.entered().await;
    abandoned.abort();
    tokio::time::timeout(Duration::from_secs(3), gate.retired())
        .await
        .expect("an abandoned request must retire");
}

#[tokio::test]
async fn handlers_run_on_arrival_even_while_the_reply_is_held() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let plugin = FakePlugin::builder(manifest())
        .get("v1/ping", {
            let calls = calls.clone();
            move |_| {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Reply::Json(json!({}))
            }
        })
        .start()
        .await;
    let gate = plugin.hold("v1/ping");
    let url = format!("{}v1/ping", plugin.url);
    let pending = tokio::spawn(async move { client().get(url).send().await.unwrap().status() });
    gate.entered().await;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    gate.release();
    assert_eq!(pending.await.unwrap(), StatusCode::OK);
}

#[tokio::test]
async fn strict_plugins_refuse_queries_malformed_bodies_and_excess_requests() {
    let plugin = FakePlugin::builder(manifest())
        .post("v1/search", |_| Reply::Json(json!({})))
        .strict(3)
        .start()
        .await;
    let url = format!("{}v1/search", plugin.url);
    let post = |url: String, body: &'static str| {
        let request = client().post(url).body(body);
        async move { request.send().await.unwrap().status() }
    };
    assert_eq!(post(url.clone(), "{}").await, StatusCode::OK);
    assert_eq!(
        post(format!("{url}?x=1"), "{}").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post(url.clone(), "{not json").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(post(url.clone(), "{}").await, StatusCode::BAD_REQUEST);
    let handled: Vec<Handled> = plugin.requests().iter().map(|r| r.handled).collect();
    assert_eq!(
        handled,
        [
            Handled::Yes,
            Handled::Refused,
            Handled::Refused,
            Handled::Refused
        ]
    );
    assert_eq!(plugin.unexpected(), 3);
}

#[tokio::test]
async fn refusing_replies_mark_the_request_and_count_as_unexpected() {
    let plugin = FakePlugin::builder(manifest())
        .post("v1/search", |request| {
            if request.json()["ok"] == true {
                Reply::Json(json!({}))
            } else {
                Reply::Refuse
            }
        })
        .start()
        .await;
    let url = format!("{}v1/search", plugin.url);
    for body in [json!({"ok": true}), json!({"ok": false})] {
        client().post(&url).json(&body).send().await.unwrap();
    }
    client()
        .post(format!("{}v1/other", plugin.url))
        .send()
        .await
        .unwrap();
    let handled: Vec<Handled> = plugin.requests().iter().map(|r| r.handled).collect();
    assert_eq!(handled, [Handled::Yes, Handled::Refused, Handled::No]);
    assert_eq!(plugin.unexpected(), 2);
}
