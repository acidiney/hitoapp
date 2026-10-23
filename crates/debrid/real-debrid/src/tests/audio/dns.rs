mod answers;
mod api;
mod destination_tls;
mod destinations;
mod fixture;
mod mixed;
mod preflight;
mod protocol;

use super::{
    CALLER_TOKEN, EXPECTED_FORM, FILE, HASH, HOSTER_LINK, MEDIA_TARGET, SERVICE_TOKEN, TITLE,
    TOKEN, TORRENT, WAV_BYTES,
    tls::{API_HOST, DNS_MEDIA_HOST as MEDIA_HOST, Fixture, Origin},
    wave,
};
use crate::tests::logging::Capture;
use crate::{Error, MediaAllowlist, RealDebridClient, TorrentFileBinding};
use fixture::{DnsFixture, Question};
use serde_json::json;
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};
use tokio::task::{AbortHandle, JoinSet};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn media_dns_stall_cancellation_and_deadline_retire_queries_before_full_admission_recovery() {
    let tls = Fixture::with_dns_media(api_replies(), wave()).await;
    let dns = DnsFixture::start().await;
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: WAV_BYTES as u64,
    };
    let original = binding.clone();
    let capture = Capture::default();
    let mut operations = JoinSet::new();
    let result = tokio::time::timeout(Duration::from_millis(13_500), async {
        let client: RealDebridClient = RealDebridClient::with_test_tls_dns(
            TOKEN.into(), tls.ca_der(), tls.api_address, (MEDIA_HOST, tls.media_address), dns.address,
        ).map_err(|_| "the fixed-host owned DNS client must initialize")?;
        let allowlist = MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")])
            .map_err(|_| "the exact owned media allowlist must initialize")?;
        let first = start(&mut operations, &client, &binding, &capture);
        let first_questions = dns.questions(2).await;
        let first_connections = address_round(&first_questions[..2])?;
        let second_started = Instant::now();
        let second = start(&mut operations, &client, &binding, &capture);
        let second_questions = dns.questions(4).await;
        let second_connections = address_round(&second_questions[2..4])?;
        if !first_connections.is_disjoint(&second_connections) {
            return Err("separate opening operations must own separate stalled DNS connections");
        }
        let busy = tokio::time::timeout(Duration::from_secs(1), client.open_bound_audio(&binding, &allowlist, crate::AudioKind::Pcm16Wav))
            .await.map_err(|_| "capacity rejection must remain prompt while both DNS operations are held")?;
        let busy_error = busy.err();
        first.abort();
        let cancelled = operations.join_next().await.ok_or("the cancelled operation must remain owned")?;
        let cancelled = matches!(cancelled, Err(error) if error.is_cancelled() && error.id() == first.id());
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&first_connections)).await
            .map_err(|_| "caller cancellation must close every first-operation DNS socket while the client remains alive")?;
        let deadline = operations.join_next().await.ok_or("the deadline operation must remain owned")?;
        let deadline_error = match deadline {
            Ok(result) => result.err(),
            Err(_) => return Err("the second opening must return its typed deadline without task failure"),
        };
        let deadline_elapsed = second_started.elapsed();
        if !second.is_finished() { return Err("the deadline operation must have retired before capacity reuse"); }
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&second_connections)).await
            .map_err(|_| "the aggregate deadline must retire all second-operation DNS sockets")?;

        let third = start(&mut operations, &client, &binding, &capture);
        let third_questions = dns.questions(6).await;
        let third_connections = address_round(&third_questions[4..6])?;
        let fourth = start(&mut operations, &client, &binding, &capture);
        let fourth_questions = dns.questions(8).await;
        let fourth_connections = address_round(&fourth_questions[6..8])?;
        if !third_connections.is_disjoint(&fourth_connections) {
            return Err("both recovered openings must independently reach held DNS");
        }
        third.abort();
        fourth.abort();
        let mut recovery_cancelled = 0;
        while let Some(result) = operations.join_next().await {
            if matches!(result, Err(error) if error.is_cancelled() && (error.id() == third.id() || error.id() == fourth.id())) {
                recovery_cancelled += 1;
            } else {
                return Err("both recovered opening tasks must retire by owned cancellation");
            }
        }
        let recovered_connections = third_connections.union(&fourth_connections).copied().collect();
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&recovered_connections)).await
            .map_err(|_| "both recovered DNS scopes must close all their sockets while the client remains alive")?;
        Ok((busy_error, cancelled, deadline_error, deadline_elapsed, recovery_cancelled))
    }).with_subscriber(capture.dispatch()).await;
    operations.abort_all();
    while operations.join_next().await.is_some() {}
    let dns_report = dns.close().await;
    let received = tls.close().await;

    let (busy_error, cancelled, deadline_error, elapsed, recovery_cancelled) = result
        .expect("the owned DNS lifecycle must finish within its aggregate guard")
        .expect("the owned DNS lifecycle setup and barriers must complete");
    assert!(
        binding == original,
        "DNS failure must preserve the immutable bound identity"
    );
    assert!(
        busy_error == Some(Error::Busy),
        "two stalled opening scopes must occupy both clone-shared media permits"
    );
    assert!(
        cancelled && recovery_cancelled == 2,
        "all caller-cancelled operation tasks must be joined"
    );
    assert!(
        deadline_error == Some(Error::Deadline),
        "DNS after a validated acknowledgement must reach the aggregate opening deadline without becoming uncertain"
    );
    assert!(
        (Duration::from_millis(8_500)..Duration::from_millis(10_500)).contains(&elapsed),
        "a held DNS response must be bounded by the nine-second opening budget"
    );
    assert!(
        dns_report.joined && !dns_report.forced && !dns_report.failed,
        "all DNS fixture handlers must retire and join without forced cleanup"
    );
    assert_eq!(
        dns_report.questions.len(),
        8,
        "four admitted operations must each send exactly their bounded A and AAAA questions without retry"
    );
    assert!((4..=8).contains(&dns_report.accepted));
    assert_eq!(
        dns_report.peer_closed.len(),
        dns_report.accepted,
        "no DNS connection may survive cancelled or expired owned operations"
    );
    assert_eq!(
        received.len(),
        8,
        "the rejected third admission must create no extra API or media HTTP request"
    );
    for (index, observed) in received.iter().enumerate() {
        assert!(
            observed.origin == Origin::Api && observed.server_name == API_HOST,
            "all HTTP requests must remain on the fixed verified API origin while DNS is stalled"
        );
        let request = &observed.request;
        let (method, target) = if index.is_multiple_of(2) {
            ("GET", format!("/rest/1.0/torrents/info/{TORRENT}"))
        } else {
            ("POST", "/rest/1.0/unrestrict/link".into())
        };
        assert!(request.method == method && request.target == target);
        assert!(request.header("host") == Some(API_HOST));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert!(if index.is_multiple_of(2) {
            request.body.is_empty()
        } else {
            request.body.as_slice() == EXPECTED_FORM.as_bytes()
        });
        for header in [
            "cookie",
            "proxy-authorization",
            "x-hito-plugin-protocol",
            "range",
            "transfer-encoding",
        ] {
            assert!(request.header(header).is_none());
        }
        for (_, value) in &request.headers {
            assert!(!value.contains(SERVICE_TOKEN) && !value.contains(CALLER_TOKEN));
        }
    }
    let logs = capture.text();
    let events: Vec<serde_json::Value> = logs
        .lines()
        .map(|line| serde_json::from_str(line).expect("owned DNS workflow logs must remain JSON"))
        .collect();
    assert!(
        events.iter().any(
            |event| event["fields"]["event"] == "real_debrid_audio_opening_finished"
                && event["fields"]["outcome"] == "deadline"
        ),
        "the aggregate DNS timeout must have a safe typed terminal outcome"
    );
    assert!(events.iter().any(|event| event["fields"]["event"]
        == "real_debrid_audio_opening_finished"
        && event["fields"]["outcome"] == "busy"));
    for required in ["request_id", "operation", "outcome", "elapsed_ms"] {
        assert!(logs.contains(required));
    }
    assert!(!logs.contains("unrestriction_uncertain"));
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
        MEDIA_HOST,
        "private-",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private),
            "DNS lifecycle logs must not contain private binding, credential, hoster or generated media values"
        );
    }
}

fn start(
    operations: &mut JoinSet<Result<(), Error>>,
    client: &RealDebridClient,
    binding: &TorrentFileBinding,
    capture: &Capture,
) -> AbortHandle {
    let client = client.clone();
    let binding = binding.clone();
    operations.spawn(
        async move {
            let allowlist = MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")])?;
            client
                .open_bound_audio(&binding, &allowlist, crate::AudioKind::Pcm16Wav)
                .await
                .map(drop)
        }
        .with_subscriber(capture.dispatch()),
    )
}

fn address_round(questions: &[Question]) -> Result<BTreeSet<usize>, &'static str> {
    if questions.len() != 2
        || questions
            .iter()
            .map(|question| question.record_type)
            .collect::<BTreeSet<_>>()
            != BTreeSet::from([1, 28])
    {
        return Err("each owned resolution must issue only the exact A and AAAA questions");
    }
    Ok(questions
        .iter()
        .map(|question| question.connection)
        .collect())
}

fn api_replies() -> Vec<Vec<u8>> {
    let download = format!("https://{MEDIA_HOST}{MEDIA_TARGET}");
    let metadata = serde_json::to_vec(&json!({
        "id": TORRENT, "filename": TITLE, "hash": HASH, "bytes": WAV_BYTES,
        "progress": 100, "status": "downloaded",
        "files": [{"id": 7, "path": FILE, "bytes": WAV_BYTES, "selected": 1}], "links": [HOSTER_LINK]
    })).expect("the owned DNS metadata reply must serialize");
    let acknowledgement = serde_json::to_vec(&json!({
        "id": "private-dns-download-id-canary", "filesize": WAV_BYTES, "link": HOSTER_LINK, "download": download
    })).expect("the owned DNS acknowledgement must serialize");
    (0..4)
        .flat_map(|_| [metadata.clone(), acknowledgement.clone()])
        .collect()
}
