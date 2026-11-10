use crate::{
    fixture::{AudioFixture, no_configuration},
    tones,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::Credentials;
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn quality_queries_are_typed_bounded_and_authenticated() {
    let fixture = AudioFixture::start("flac", |path| tones::flac(path, 5), no_configuration).await;
    for query in [
        "quality=PrivateSecret",
        "quality=HIGH",
        "quality=saver&quality=high",
        "start_ms=-1",
        "start_ms=1.5",
        "start_ms=18446744073709551616",
        "quality=saver&private=Secret",
    ] {
        for method in ["GET", "HEAD"] {
            let response = fixture.response(method, &format!("?{query}")).await;
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{method} {query}"
            );
            if method == "GET" {
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                let text = String::from_utf8(bytes.to_vec()).unwrap();
                assert!(!text.contains("Secret"));
            }
        }
    }
    for method in ["GET", "HEAD"] {
        let response = fixture
            .response(method, "?quality=saver&start_ms=604800001")
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!(
                        "/api/v1/sources/{}/audio?quality=saver",
                        fixture.source
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let boundary = fixture
        .response("HEAD", "?quality=saver&start_ms=604800000")
        .await;
    assert_eq!(boundary.status(), StatusCode::OK);
    assert_eq!(boundary.headers()["x-hito-stream-start-ms"], "604800000");
    let actor = fixture
        .services
        .identity
        .authenticate(&fixture.token)
        .await
        .unwrap();
    fixture
        .services
        .identity
        .create_user(
            &actor,
            Credentials {
                username: "reader".into(),
                password: "private reader password".into(),
            },
        )
        .await
        .unwrap();
    let reader = fixture
        .services
        .identity
        .login(Credentials {
            username: "reader".into(),
            password: "private reader password".into(),
        })
        .await
        .unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/sources/{}/audio?quality=saver",
                    fixture.source
                ))
                .header("authorization", format!("Bearer {}", reader.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
