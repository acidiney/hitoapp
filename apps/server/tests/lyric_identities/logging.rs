use super::setup::Prepared;
use crate::logs;
use axum::http::StatusCode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn private_lyric_workflows_emit_correlated_static_outcomes_without_private_inputs_or_results()
{
    let prepared = Prepared::new().await;
    let captured = logs::Logs::default();
    let (stamp,digest)=async {
        for suffix in ["services","identities"] {
            let (status,headers,_)=crate::account::exchange(&prepared.library.app,"GET",&format!("{}/{suffix}",prepared.base()),Value::Null,prepared.library.account.headers()).await;
            assert_eq!(status,StatusCode::OK);assert_eq!(headers["cache-control"],"private, no-store");
        }
        let (status,_)=prepared.library.account.request(&prepared.library.app,"POST",&format!("{}/candidates",prepared.base()),json!({"installation_id":prepared.installation["id"],"installation_revision":prepared.installation["revision"],"query":"Explicit private search"})).await;
        assert_eq!(status,StatusCode::OK);
        let (status,review)=prepared.library.account.request(&prepared.library.app,"POST",&format!("{}/identities/review",prepared.base()),prepared.candidate.clone()).await;
        assert_eq!(status,StatusCode::OK);
        let (status,_)=prepared.library.account.request(&prepared.library.app,"POST",&format!("{}/identities",prepared.base()),json!({"candidate":prepared.candidate,"review_stamp":review["review_stamp"]})).await;
        assert_eq!(status,StatusCode::OK);
        let read=prepared.read().await;
        let (status,_)=prepared.library.account.request(&prepared.library.app,"DELETE",&format!("{}/identities/{}?revision=1",prepared.base(),prepared.identity["selector"]["identity_id"].as_str().unwrap()),Value::Null).await;
        assert_eq!(status,StatusCode::NO_CONTENT);
        (review["review_stamp"].as_str().unwrap().to_owned(),read["document_digest"].as_str().unwrap().to_owned())
    }.with_subscriber(captured.subscriber()).await;
    let headers = prepared.library.account.headers();
    let cookie = headers["cookie"].to_str().unwrap();
    let token = cookie.strip_prefix("hito_session=").unwrap();
    let session_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    captured.assert_private(
        &[
            "Explicit private search",
            "Owned independent title",
            "Owned independent artist",
            "Owned untimed lyric text",
            "independent:recording",
            &prepared.provider.url,
            &prepared.library.recording,
            prepared.identity["selector"]["identity_id"]
                .as_str()
                .unwrap(),
            cookie,
            token,
            &session_hash,
            &stamp,
            &digest,
        ],
        &[],
    );
    captured.assert_identity_events(&[
        ("lyric_services", "completed"),
        ("lyric_identities", "completed"),
        ("lyric_candidates", "completed"),
        ("lyric_identity_review", "completed"),
        ("lyric_identity_retain", "completed"),
        ("selected_recording_lyrics", "ready"),
        ("lyric_identity_remove", "completed"),
    ]);
    prepared.library.assert_original();
    prepared.provider.finish().await;
}
