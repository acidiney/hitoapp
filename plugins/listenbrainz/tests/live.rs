use hito_plugin_listenbrainz::{Config, router};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use uuid::Uuid;

const TOKEN: &str = "service-token-for-live-check";
const RADIOHEAD: &str = "a74b1b7f-71a5-4011-9441-d0b5e4122711";

#[tokio::test]
#[ignore = "Requires HITO_LISTENBRAINZ_LIVE=1, HITO_LISTENBRAINZ_CONTACT and the public ListenBrainz labs service"]
async fn the_plugin_reads_similar_artists_from_the_live_labs_service() {
    assert!(
        std::env::var("HITO_LISTENBRAINZ_LIVE").as_deref() == Ok("1"),
        "ListenBrainz live check failed: stage=opt_in code=explicit_opt_in_required"
    );
    let contact = std::env::var("HITO_LISTENBRAINZ_CONTACT")
        .expect("ListenBrainz live check failed: stage=contact code=operator_contact_required");
    let started = Instant::now();
    let config = Config::new(&contact, TOKEN).expect("invalid operator contact");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let plugin = format!("http://{}/", listener.local_addr().unwrap());
    let app = router(config).unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let response = tokio::time::timeout(
        Duration::from_secs(30),
        reqwest::Client::new()
            .post(format!("{plugin}v1/catalog/similarity/artists"))
            .bearer_auth(TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .json(&json!({"artist_id": RADIOHEAD, "limit": 5}))
            .send(),
    )
    .await
    .expect("ListenBrainz live check failed: stage=similarity code=deadline_exceeded")
    .expect("ListenBrainz live check failed: stage=similarity code=transport_error");
    let status = response.status().as_u16();
    assert_eq!(
        status, 200,
        "ListenBrainz live check failed: stage=similarity code=plugin_status http_status={status}"
    );
    let page: Value = response.json().await.unwrap();
    assert_eq!(page["artist_id"], RADIOHEAD);
    let items = page["items"].as_array().unwrap();
    assert!(
        !items.is_empty() && items.len() <= 5,
        "ListenBrainz live check failed: stage=similarity code=unexpected_item_count count={}",
        items.len()
    );
    let mut seen = std::collections::HashSet::new();
    for item in items {
        let id = item["artist"]["id"].as_str().unwrap();
        assert!(
            Uuid::parse_str(id).is_ok_and(|parsed| parsed.hyphenated().to_string() == id),
            "similar artist identity not canonical"
        );
        assert_ne!(id, RADIOHEAD);
        assert!(seen.insert(id.to_owned()), "repeated similar artist");
        assert!(!item["artist"]["name"].as_str().unwrap().is_empty());
        assert!(item["score"].is_number() || item["score"].is_null());
    }
    tracing::info!(
        workflow = "listenbrainz_live_similarity",
        outcome = "success",
        items = items.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "ListenBrainz live similarity check completed"
    );
}
