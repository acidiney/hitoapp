use super::ARTIST_ID;
use super::http::{Capture, Fixture, Reply};
use crate::{Error, MusicBrainzClient};
use axum::http::StatusCode;
use hito_plugin_sdk::CatalogArtistProfile;
use serde_json::json;

const PRIVATE_NAME: &str = "private-artist-canary!";
const PRIVATE_BODY: &str = "private-error-body-canary";
const PRIVATE_URL: &str =
    "https://provider.invalid/private-url-canary?signature=private-signature-canary";

fn profile(name: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "id": ARTIST_ID,
        "name": name,
        "private_provider_field": PRIVATE_URL
    }))
    .expect("the owned profile must serialize")
}

fn assert_safe_logs(capture: &Capture, endpoint: &reqwest::Url) {
    let logs = capture.text();
    for required in [
        "MusicBrainz artist profile completed",
        "request_id",
        "outcome",
        "elapsed_ms",
    ] {
        assert!(
            logs.contains(required),
            "the workflow log capture must be active"
        );
    }
    for private in [
        PRIVATE_NAME,
        PRIVATE_BODY,
        PRIVATE_URL,
        "private-signature-canary",
        "private-redirect-canary",
        endpoint.as_str(),
    ] {
        assert!(
            !logs.contains(private),
            "adapter workflow logs must exclude private protocol data"
        );
    }
}

async fn fetch(reply: Reply) -> Result<CatalogArtistProfile, Error> {
    let fixture = Fixture::start(vec![reply]).await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned protocol client must initialize");
    let capture = Capture::default();
    let result = capture.profile(&client, ARTIST_ID.into()).await;
    drop(client);
    let request_count = fixture.request_count();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;
    assert_eq!(
        request_count, 1,
        "the adapter must not retry or follow redirects"
    );
    assert_safe_logs(&capture, &endpoint);
    result
}

#[tokio::test]
async fn profile_wire_cap_accepts_exact_limit_and_rejects_declared_or_streamed_overflow() {
    for streamed in [false, true] {
        let mut exact = profile(PRIVATE_NAME);
        exact.resize(65_536, b' ');
        let mut reply = Reply::json(exact.clone());
        reply.streamed = streamed;
        let accepted = fetch(reply)
            .await
            .expect("an exact 64KiB profile must be accepted");
        assert!(accepted.artist.name == PRIVATE_NAME);

        exact.push(b' ');
        let mut reply = Reply::json(exact);
        reply.streamed = streamed;
        let error = fetch(reply)
            .await
            .err()
            .expect("a profile above 64KiB must be rejected");
        assert_eq!(error, Error::ResponseTooLarge);
        assert_eq!(
            error.to_string(),
            "The MusicBrainz response exceeds the size limit"
        );
    }
}

#[tokio::test]
async fn profile_rejects_wrong_mime_redirects_and_private_status_bodies() {
    let mut wrong_mime = Reply::json(profile(PRIVATE_NAME));
    wrong_mime.mime = "text/html";
    let error = fetch(wrong_mime)
        .await
        .err()
        .expect("HTML must not become an artist profile");
    assert_eq!(error, Error::InvalidResponse);
    assert_eq!(
        error.to_string(),
        "The MusicBrainz service returned an invalid artist profile"
    );

    let malformed = Reply::json(format!("{PRIVATE_BODY} {PRIVATE_URL}").into_bytes());
    let error = fetch(malformed)
        .await
        .err()
        .expect("malformed JSON must remain a static profile error");
    assert_eq!(error, Error::InvalidResponse);
    assert_eq!(
        error.to_string(),
        "The MusicBrainz service returned an invalid artist profile"
    );

    let mut redirect = Reply::json(profile(PRIVATE_NAME));
    redirect.status = StatusCode::FOUND;
    redirect.location = Some("/private-redirect-canary?signature=private-signature-canary");
    let error = fetch(redirect)
        .await
        .err()
        .expect("redirects must remain disabled");
    assert_eq!(error, Error::Status(302));

    let mut rejected = Reply::json(format!("{PRIVATE_BODY} {PRIVATE_URL}").into_bytes());
    rejected.status = StatusCode::SERVICE_UNAVAILABLE;
    let error = fetch(rejected)
        .await
        .err()
        .expect("provider errors must remain typed and redacted");
    assert_eq!(error, Error::Status(503));
    assert_eq!(
        error.to_string(),
        "The MusicBrainz service rejected the request"
    );
    assert_eq!(format!("{error:?}"), "Status(503)");
}

#[tokio::test]
async fn profile_rejects_noncanonical_ids_before_http_and_enforces_utf8_name_bytes() {
    let mut exact_name = PRIVATE_NAME.to_owned();
    exact_name.push_str(&"é".repeat((2_048 - PRIVATE_NAME.len()) / 2));
    if exact_name.len() < 2_048 {
        exact_name.push('x');
    }
    assert_eq!(exact_name.len(), 2_048);
    let overflow_name = format!("{exact_name}é");
    let fixture = Fixture::start(vec![
        Reply::json(profile(&exact_name)),
        Reply::json(profile(&overflow_name)),
    ])
    .await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned identity-boundary client must initialize");
    let capture = Capture::default();
    let mut rejected = Vec::new();
    for id in [
        ARTIST_ID.to_uppercase(),
        ARTIST_ID.replace('-', ""),
        format!("{ARTIST_ID}/child"),
        PRIVATE_URL.into(),
    ] {
        rejected.push(capture.profile(&client, id).await);
    }
    let before = fixture.request_count();
    let accepted = capture.profile(&client, ARTIST_ID.into()).await;
    let overflow = capture.profile(&client, ARTIST_ID.into()).await;
    drop(client);
    let after = fixture.request_count();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;

    for result in rejected {
        let error = result
            .err()
            .expect("noncanonical artist identities must be rejected");
        assert_eq!(error, Error::InvalidRequest);
        assert_eq!(
            error.to_string(),
            "Use a canonical MusicBrainz artist identifier"
        );
    }
    assert_eq!(
        before, 0,
        "invalid artist identifiers must not make provider requests"
    );
    assert_eq!(after, 2);
    assert!(
        accepted
            .expect("the exact UTF-8 name boundary must be accepted")
            .artist
            .name
            == exact_name
    );
    assert_eq!(
        overflow
            .err()
            .expect("UTF-8 byte overflow must be rejected"),
        Error::InvalidResponse
    );
    assert_safe_logs(&capture, &endpoint);
}
