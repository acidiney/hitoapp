use super::fixture::Case;
use axum::{body::to_bytes, http::StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

async fn json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap()).unwrap()
}

#[tokio::test]
async fn a_client_gets_back_the_lease_it_lost_the_answer_to_and_frees_its_own_after_a_restart() {
    let f = Case::new().await;
    let request = Uuid::new_v4();
    let body = serde_json::to_vec(&json!({
        "recording_id": f.recording,
        "source_id": f.source,
        "client": "phone-install",
        "request_id": request,
    }))
    .unwrap();
    let first = f
        .request("POST", "/api/v1/playback/leases", &[], Some(body.clone()))
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    let first = json(first).await;
    let again = f
        .request("POST", "/api/v1/playback/leases", &[], Some(body))
        .await;
    assert_eq!(again.status(), StatusCode::OK);
    assert_eq!(json(again).await, first);

    let untagged = json(f.begin().await).await;
    let released = f
        .request(
            "DELETE",
            "/api/v1/playback/leases?client=phone-install",
            &[],
            None,
        )
        .await;
    assert_eq!(released.status(), StatusCode::NO_CONTENT);
    let gone = f
        .request(
            "GET",
            &format!("/api/v1/playback/leases/{}", first["id"].as_str().unwrap()),
            &[],
            None,
        )
        .await;
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    let kept = f
        .request(
            "GET",
            &format!(
                "/api/v1/playback/leases/{}",
                untagged["id"].as_str().unwrap()
            ),
            &[],
            None,
        )
        .await;
    assert_eq!(kept.status(), StatusCode::OK);

    let anonymous = f
        .request_as(
            None,
            "DELETE",
            "/api/v1/playback/leases?client=phone-install",
            &[],
            None,
        )
        .await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    let missing = f
        .request("DELETE", "/api/v1/playback/leases", &[], None)
        .await;
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    f.close().await;
}
