use super::fixture::{Fixture, ROOT};
use axum::http::StatusCode;
use hito_core::test_support::support::write_wav;
use serde_json::Value;
use std::time::Duration;

#[tokio::test]
async fn expired_snapshots_are_hidden_and_a_fresh_snapshot_remains_readable() {
    let f = Fixture::start().await;
    write_wav(&f.music.join("Owned.wav"), 0);
    f.scan().await;
    let initial = f.begin().await;
    let id = initial["id"].as_str().unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(299)).await;
    tokio::time::resume();
    assert_eq!(
        f.begin().await,
        initial,
        "Resuming must retain the original expiry"
    );
    assert_eq!(
        f.call("GET", &format!("{ROOT}/{id}/pages/0"), Value::Null)
            .await
            .status,
        StatusCode::OK
    );
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(2)).await;
    tokio::time::resume();
    let expired = f
        .call("GET", &format!("{ROOT}/{id}/pages/0"), Value::Null)
        .await;
    assert_eq!(expired.status, StatusCode::NOT_FOUND);
    let fresh = f.begin().await;
    assert_ne!(fresh["id"], initial["id"]);
    let page = f
        .call(
            "GET",
            &format!("{ROOT}/{}/pages/0", fresh["id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
    assert_eq!(
        page.status,
        StatusCode::OK,
        "A fresh handle after expiry must still have its full lifetime"
    );
    assert_eq!(page.json()["recordings"].as_array().unwrap().len(), 1);
    let release = f.call("DELETE", &format!("{ROOT}/{id}"), Value::Null).await;
    assert_eq!(release.status, StatusCode::NO_CONTENT);
    assert!(release.bytes.is_empty());
    f.services.catalog_replication.close().await.unwrap();
}
