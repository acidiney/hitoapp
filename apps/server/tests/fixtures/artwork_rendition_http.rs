use crate::{
    artwork_normalization_fixture::{Context, Library},
    artwork_normalization_provider as provider,
    artwork_rendition_images::Case,
};
use axum::http::{HeaderMap, Method, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

pub struct Captured {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub source_bytes: usize,
}

pub async fn render(case: Case) -> Captured {
    let mut library = Library::retain();
    let mut source_bytes = 0;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let context = hang_guard(
            "owned artwork format HTTP setup",
            Box::pin(async {
                let (bytes, content_type) = case.source();
                source_bytes = bytes.len();
                library.open().await;
                library.use_cover(bytes, content_type);
                Box::pin(library.prepare()).await
            }),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(exercise(&library, &context)),
        )
        .await
        .expect("The owned artwork format workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    library.close().await;
    let (bytes, content_type) = match outcome {
        Ok(captured) => captured,
        Err(panic) => resume_unwind(panic),
    };
    Captured {
        bytes,
        content_type,
        source_bytes,
    }
}

async fn exercise(library: &Library, context: &Context) -> (Vec<u8>, String) {
    let path = format!("/api/v1/releases/{}/artwork", context.release);
    let before = library.snapshot(context).await;
    library.assert_originals();
    let requests = library.provider().requests().len();
    for (method, uri) in [("GET", path.clone()), ("POST", format!("{path}/fetch"))] {
        let denied = library
            .exchange(Some(&context.peer), method, &uri, Value::Null)
            .await;
        assert_eq!(denied.status, StatusCode::NOT_FOUND);
        private(&denied.headers);
    }
    assert_eq!(library.provider().requests().len(), requests);
    let queued = library
        .exchange(
            Some(&context.owner),
            "POST",
            &format!("{path}/fetch"),
            Value::Null,
        )
        .await;
    assert_eq!(queued.status, StatusCode::ACCEPTED);
    private(&queued.headers);
    assert_eq!(queued.json()["status"], "queued");
    let id = queued.json()["id"].as_str().unwrap().to_owned();
    assert!(
        library
            .hito()
            .catalog
            .run_next_artwork_job_until(std::future::pending())
            .await
            .unwrap()
    );
    let ready = library
        .get(
            &context.owner,
            &format!("/api/v1/release-artwork-fetches/{id}"),
        )
        .await;
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["attempts"], 1);
    assert!(ready["error"].is_null());
    let image = library
        .exchange(Some(&context.owner), "GET", &path, Value::Null)
        .await;
    assert_eq!(image.status, StatusCode::OK);
    private(&image.headers);
    let retained = library
        .exchange(Some(&context.owner), "GET", &path, Value::Null)
        .await;
    assert_eq!(retained.status, StatusCode::OK);
    assert_eq!(retained.bytes, image.bytes);
    private(&retained.headers);
    let denied = library
        .exchange(Some(&context.peer), "GET", &path, Value::Null)
        .await;
    assert_eq!(denied.status, StatusCode::NOT_FOUND);
    private(&denied.headers);
    assert_eq!(library.snapshot(context).await, before);
    library.assert_originals();
    let calls: Vec<_> = library.provider().requests()[requests..]
        .iter()
        .map(|request| {
            (
                request.method.clone(),
                request.operation.clone(),
                request.json(),
            )
        })
        .collect();
    assert_eq!(
        calls,
        vec![(
            Method::POST,
            "v1/artwork".into(),
            json!({"entity": "release", "provider_id": provider::RELEASE})
        )]
    );
    assert_eq!(library.provider().unexpected(), 0);
    (
        image.bytes,
        image.headers["content-type"].to_str().unwrap().to_owned(),
    )
}

fn private(headers: &HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}
