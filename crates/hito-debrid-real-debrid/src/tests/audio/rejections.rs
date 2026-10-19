mod cases;

use super::{
    CALLER_TOKEN, EXPECTED_FORM, FILE, HASH, HOSTER_LINK, MEDIA_TARGET, SERVICE_TOKEN, TITLE,
    TOKEN, TORRENT,
    tls::{API_HOST, Fixture, MEDIA_HOST, Origin, Received},
    wave,
};
use crate::tests::logging::Capture;
use crate::{Error, MediaAllowlist, RealDebridClient, TorrentFileBinding};
use cases::{Expected, cases};
use serde_json::json;
use std::time::Duration;
use tracing::instrument::WithSubscriber;

const LENGTH: usize = 76;

struct Observed {
    error: Option<Error>,
    returned_binding: Option<TorrentFileBinding>,
    bytes: Vec<u8>,
    bounded_pulls: bool,
}

#[test]
fn rejects_nonexact_operator_media_origins() {
    for origin in [
        "http://media.hito-fixture.invalid",
        "https://media.hito-fixture.invalid:8443",
        "https://127.0.0.1",
        "https://[::1]",
        "https://private-user@media.hito-fixture.invalid",
        "https://@media.hito-fixture.invalid",
        "https://*.hito-fixture.invalid",
        "https://media.hito-fixture.invalid/private-path",
        "https://media.hito-fixture.invalid?private-query",
        "https://media.hito-fixture.invalid#private-fragment",
    ] {
        assert!(
            matches!(
                MediaAllowlist::new(vec![origin.into()]),
                Err(Error::Configuration)
            ),
            "operator media configuration must contain only exact HTTPS DNS origins on port 443"
        );
    }
}

#[tokio::test]
async fn known_acknowledgements_keep_destination_and_media_failures_distinct_from_uncertainty() {
    let canonical = short_wave();
    for case in cases(&canonical) {
        let binding = binding();
        let original = binding.clone();
        let api = api_replies(&binding, &case.download);
        let fixture = Fixture::with_media_reply(api, case.response).await;
        let capture = Capture::default();
        let result = tokio::time::timeout(Duration::from_secs(4), async {
            let client = RealDebridClient::with_test_tls(
                TOKEN.into(),
                fixture.ca_der(),
                fixture.api_address,
                (MEDIA_HOST, fixture.media_address),
            )
            .map_err(|_| "the owned rejection client must initialize")?;
            let allowlist = MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")])
                .map_err(|_| "the exact owned media allowlist must initialize")?;
            let mut audio = match client.open_bound_audio(&binding, &allowlist).await {
                Ok(audio) => audio,
                Err(error) => {
                    return Ok(Observed {
                        error: Some(error),
                        returned_binding: None,
                        bytes: Vec::new(),
                        bounded_pulls: true,
                    });
                }
            };
            let returned_binding = Some(audio.binding.clone());
            let mut bytes = Vec::new();
            let mut bounded_pulls = true;
            let error = loop {
                match audio.next_chunk().await {
                    Ok(Some(chunk)) => {
                        if chunk.is_empty()
                            || chunk.len() > 65_536
                            || chunk.len() > LENGTH.saturating_sub(bytes.len())
                        {
                            bounded_pulls = false;
                            break None;
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    Ok(None) => break None,
                    Err(error) => break Some(error),
                }
            };
            Ok::<_, &'static str>(Observed {
                error,
                returned_binding,
                bytes,
                bounded_pulls,
            })
        })
        .with_subscriber(capture.dispatch())
        .await;
        let received = fixture.close().await;

        let observed = result
            .expect("the owned rejection operation must finish within its guard")
            .expect("the owned rejection setup must succeed");
        assert!(
            binding == original,
            "the bound input must remain unchanged in every rejection case"
        );
        assert!(
            observed.bounded_pulls,
            "the media body must never expose oversized or out-of-bounds pulls"
        );
        if let Some(returned_binding) = &observed.returned_binding {
            assert!(
                *returned_binding == original,
                "an opened body must preserve the exact original binding"
            );
        }
        if case.must_open {
            assert!(
                observed.returned_binding.is_some(),
                "the owned body failure must occur during consumption after successful qualification"
            );
        }
        match case.expected {
            Expected::Rejected(expected) => assert!(
                observed.error == Some(expected),
                "the known acknowledgement rejection must preserve its exact typed media error: {}",
                case.name
            ),
            Expected::RejectedFraming => assert!(
                matches!(
                    observed.error,
                    Some(Error::MediaResponse | Error::MediaTransport)
                ),
                "ambiguous framing must fail at either the HTTP parser or media policy boundary"
            ),
            Expected::Complete => {
                assert!(
                    observed.error.is_none(),
                    "canonical bytes may be delivered when MIME is only an octet-stream hint"
                );
                assert!(
                    observed.bytes == canonical,
                    "the accepted MIME hint must not change the canonical owned bytes"
                );
            }
        }
        assert!(
            observed.error != Some(Error::UnrestrictionUncertain),
            "a validated acknowledgement must not become uncertain because media failed"
        );
        assert_requests(&received, case.media_requests);
        let logs = capture.text();
        for required in ["request_id", "operation", "outcome", "elapsed_ms"] {
            assert!(
                logs.contains(required),
                "the owned media rejection must have active structured logs"
            );
        }
        let expected_outcome = match observed.error {
            Some(Error::MediaDestination) => "media_destination",
            Some(Error::UnsupportedAudio) => "unsupported_audio",
            Some(Error::MediaResponse) => "media_response",
            Some(Error::MediaTransport) => "media_transport",
            Some(Error::MediaLength) => "media_length",
            None => "complete",
            Some(_) => "unexpected",
        };
        let events: Vec<serde_json::Value> = logs
            .lines()
            .map(|line| {
                serde_json::from_str(line)
                    .expect("the owned media logs must remain structured JSON")
            })
            .collect();
        assert_eq!(
            events
                .iter()
                .filter(|event| event["fields"]["event"] == "real_debrid_audio_opening_finished")
                .count(),
            1,
            "every owned opening must log exactly one safe terminal outcome"
        );
        assert!(
            events.iter().any(|event| {
                event["fields"]["outcome"] == expected_outcome
                    && matches!(
                        event["fields"]["event"].as_str(),
                        Some("real_debrid_audio_opening_finished" | "remote_audio_body_finished")
                    )
            }),
            "the known media rejection must log its controlled typed outcome"
        );
        assert!(
            !logs.contains("unrestriction_uncertain"),
            "known acknowledgements must have no uncertain outcome"
        );
        let safe_error = observed
            .error
            .map(|error| format!("{error:?} {error}"))
            .unwrap_or_default();
        for private in [
            TOKEN,
            SERVICE_TOKEN,
            CALLER_TOKEN,
            TORRENT,
            HASH,
            FILE,
            TITLE,
            HOSTER_LINK,
            EXPECTED_FORM,
            case.download.as_str(),
            "private-",
            "http://",
            "https://",
        ] {
            assert!(
                !logs.contains(private) && !safe_error.contains(private),
                "rejection logs and errors must exclude all private response, binding, credential and URL values"
            );
        }
    }
}

fn binding() -> TorrentFileBinding {
    TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: LENGTH as u64,
    }
}

fn api_replies(binding: &TorrentFileBinding, download: &str) -> Vec<Vec<u8>> {
    [
        json!({
            "id": TORRENT, "filename": TITLE, "hash": HASH, "bytes": binding.file_bytes,
            "progress": 100, "status": "downloaded",
            "files": [{"id": 7, "path": FILE, "bytes": binding.file_bytes, "selected": 1}],
            "links": [HOSTER_LINK]
        }),
        json!({
            "id": "private-audio-download-id-canary", "filename": TITLE,
            "filesize": binding.file_bytes, "link": HOSTER_LINK, "download": download,
            "mimeType": "audio/wav", "streamable": 1
        }),
    ]
    .iter()
    .map(|body| serde_json::to_vec(body).expect("the owned rejection API reply must serialize"))
    .collect()
}

fn short_wave() -> Vec<u8> {
    let mut bytes = wave();
    bytes.truncate(LENGTH);
    bytes[4..8].copy_from_slice(&((LENGTH - 8) as u32).to_le_bytes());
    bytes[40..44].copy_from_slice(&((LENGTH - 44) as u32).to_le_bytes());
    bytes
}

fn assert_requests(received: &[Received], media_requests: usize) {
    assert_eq!(
        received.len(),
        2 + media_requests,
        "a known acknowledgement may produce only the exact metadata GET, exact link POST and at most one direct media GET"
    );
    for (index, method, target) in [
        (0, "GET", format!("/rest/1.0/torrents/info/{TORRENT}")),
        (1, "POST", "/rest/1.0/unrestrict/link".into()),
    ] {
        let observed = &received[index];
        assert!(observed.origin == Origin::Api && observed.server_name == API_HOST);
        assert!(observed.request.method == method && observed.request.target == target);
        assert!(observed.request.header("host") == Some(API_HOST));
        assert!(
            observed.request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str())
        );
    }
    assert!(received[0].request.body.is_empty());
    assert!(received[1].request.body.as_slice() == EXPECTED_FORM.as_bytes());
    assert_eq!(
        received[1].request.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    if media_requests == 1 {
        let media = &received[2];
        assert!(media.origin == Origin::Media && media.server_name == MEDIA_HOST);
        assert!(media.request.method == "GET" && media.request.target == MEDIA_TARGET);
        assert!(media.request.header("host") == Some(MEDIA_HOST));
        assert!(media.request.header("authorization").is_none());
        assert!(media.request.body.is_empty());
        assert_eq!(media.request.header("accept-encoding"), Some("identity"));
    }
    for observed in received {
        for header in [
            "cookie",
            "proxy-authorization",
            "x-hito-plugin-protocol",
            "range",
            "transfer-encoding",
        ] {
            assert!(observed.request.header(header).is_none());
        }
        for (_, value) in &observed.request.headers {
            assert!(!value.contains(SERVICE_TOKEN) && !value.contains(CALLER_TOKEN));
        }
    }
}
