use crate::{artwork_normalization_fixture::Library, artwork_normalization_provider as provider};
use axum::http::{Method, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn managed_cover_copies_are_bounded_without_changing_owned_sources_or_private_references() {
    let mut library = Library::retain();
    let outcome = AssertUnwindSafe(Box::pin(async {
        let context = hang_guard(
            "owned artwork normalization HTTP setup",
            Box::pin(async {
                library.open().await;
                Box::pin(library.prepare()).await
            }),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(exercise(&library, &context)),
        )
        .await
        .expect("The owned artwork normalization workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    library.close().await;
    let dimensions = match outcome {
        Ok(dimensions) => dimensions,
        Err(panic) => resume_unwind(panic),
    };
    for (width, height) in dimensions {
        assert!(
            width.max(height) <= 600,
            "The retained managed rendition must have a longest edge of at most 600 px; got {width}x{height}"
        );
    }
}

async fn exercise(
    library: &Library,
    context: &crate::artwork_normalization_fixture::Context,
) -> Vec<(u32, u32)> {
    let paths = [
        format!("/api/v1/recordings/{}/artwork", context.recording),
        format!("/api/v1/releases/{}/artwork", context.release),
    ];
    let before = library.snapshot(context).await;
    library.assert_originals();
    let calls_before = library.provider().requests().len();
    for path in &paths {
        for (method, requested) in [("GET", path.clone()), ("POST", format!("{path}/fetch"))] {
            let denied = library
                .exchange(Some(&context.peer), method, &requested, Value::Null)
                .await;
            assert_eq!(denied.status, StatusCode::NOT_FOUND);
            private(&denied.headers);
        }
    }
    assert_eq!(library.provider().requests().len(), calls_before);
    let mut images = Vec::new();
    for path in &paths {
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
        let prefix = if path.contains("/recordings/") {
            "/api/v1/artwork-fetches"
        } else {
            "/api/v1/release-artwork-fetches"
        };
        let ready = library.get(&context.owner, &format!("{prefix}/{id}")).await;
        assert_eq!(ready["status"], "ready");
        let image = library
            .exchange(Some(&context.owner), "GET", path, Value::Null)
            .await;
        assert_eq!(image.status, StatusCode::OK);
        private(&image.headers);
        let format = image::guess_format(&image.bytes).unwrap();
        assert!(matches!(
            (image.headers["content-type"].to_str().unwrap(), format),
            ("image/png", image::ImageFormat::Png) | ("image/jpeg", image::ImageFormat::Jpeg)
        ));
        images.push(image.bytes);
    }
    assert_eq!(
        Sha256::digest(&images[0]),
        Sha256::digest(&images[1]),
        "The same owned image must have the same managed representation for both references"
    );
    for path in &paths {
        let denied = library
            .exchange(Some(&context.peer), "GET", path, Value::Null)
            .await;
        assert_eq!(denied.status, StatusCode::NOT_FOUND);
        private(&denied.headers);
    }
    assert_eq!(library.snapshot(context).await, before);
    library.assert_originals();
    let calls: Vec<_> = library.provider().requests()[calls_before..]
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
        vec![
            (
                Method::POST,
                "v1/artwork".into(),
                json!({"entity": "recording", "provider_id": provider::RECORDING})
            ),
            (
                Method::POST,
                "v1/artwork".into(),
                json!({"entity": "release", "provider_id": provider::RELEASE})
            ),
        ]
    );
    assert_eq!(library.provider().unexpected(), 0);
    images
        .iter()
        .map(|bytes| {
            let image = image::load_from_memory(bytes).unwrap().to_rgb8();
            let top = (image.width() / 2, image.height() * 120 / 768);
            let stripe = (image.width() * 492 / 1024, image.height() * 592 / 768);
            assert_ne!(
                image.get_pixel(top.0, top.1),
                image.get_pixel(stripe.0, stripe.1),
                "The managed cover must retain the owned image's spatial contrast"
            );
            (image.width(), image.height())
        })
        .collect()
}

fn private(headers: &axum::http::HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}
