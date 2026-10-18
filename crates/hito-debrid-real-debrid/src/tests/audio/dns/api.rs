use super::super::{
    CALLER_TOKEN, EXPECTED_FORM, FILE, HASH, HOSTER_LINK, MEDIA_TARGET, SERVICE_TOKEN, TITLE,
    TOKEN, TORRENT, WAV_BYTES,
    tls::{API_HOST, Fixture},
    wave,
};
use super::{address_round, fixture::DnsFixture};
use crate::tests::logging::Capture;
use crate::{Error, RealDebridClient, TorrentFileBinding};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};
use tokio::task::{AbortHandle, JoinSet};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn api_dns_stall_cancellation_and_connect_deadline_retire_queries_before_full_admission_recovery()
 {
    let tls = Fixture::start(Vec::new(), wave()).await;
    let dns = DnsFixture::for_api().await;
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
    let result = tokio::time::timeout(Duration::from_millis(6_500), async {
        let client: RealDebridClient = RealDebridClient::with_test_api_dns(
            TOKEN.into(), tls.ca_der(), tls.api_address, dns.address,
        ).map_err(|_| "the fixed API host, owned CA and loopback DNS client must initialize")?;
        let retained = client.clone();
        let first = start(&mut operations, &client, &binding, &capture);
        let first_questions = dns.questions(2).await;
        let first_connections = address_round(&first_questions[..2])?;
        let second_started = Instant::now();
        let second = start(&mut operations, &retained, &binding, &capture);
        let second_questions = dns.questions(4).await;
        let second_connections = address_round(&second_questions[2..4])?;
        if !first_connections.is_disjoint(&second_connections) {
            return Err("each admitted API operation must own its stalled DNS connections");
        }
        let busy = tokio::time::timeout(Duration::from_secs(1), retained.observe(&binding)).await
            .map_err(|_| "both occupied API permits must reject another observation promptly")?;
        let busy_error = busy.err();
        first.abort();
        let cancelled = operations.join_next().await.ok_or("the cancelled API operation must remain owned")?;
        let cancelled = matches!(cancelled, Err(error) if error.is_cancelled() && error.id() == first.id());
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&first_connections)).await
            .map_err(|_| "caller cancellation must close every first-operation API DNS socket while clones remain alive")?;
        let deadline = operations.join_next().await.ok_or("the API deadline operation must remain owned")?;
        let deadline_error = match deadline {
            Ok(result) => result.err(),
            Err(_) => return Err("API DNS expiration must return its typed error without task failure"),
        };
        let elapsed = second_started.elapsed();
        if !second.is_finished() {
            return Err("the API deadline operation must retire before admission recovery");
        }
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&second_connections)).await
            .map_err(|_| "the API connection deadline must retire every second-operation DNS socket")?;

        let third = start(&mut operations, &client, &binding, &capture);
        let third_questions = dns.questions(6).await;
        let third_connections = address_round(&third_questions[4..6])?;
        let fourth = start(&mut operations, &retained, &binding, &capture);
        let fourth_questions = dns.questions(8).await;
        let fourth_connections = address_round(&fourth_questions[6..8])?;
        if !third_connections.is_disjoint(&fourth_connections) {
            return Err("both recovered API permits must independently reach held DNS");
        }
        third.abort();
        fourth.abort();
        let mut recovery_cancelled = 0;
        while let Some(result) = operations.join_next().await {
            if matches!(result, Err(error) if error.is_cancelled() && (error.id() == third.id() || error.id() == fourth.id())) {
                recovery_cancelled += 1;
            } else {
                return Err("both recovered API operation tasks must retire by owned cancellation");
            }
        }
        let recovered_connections: BTreeSet<_> = third_connections.union(&fourth_connections).copied().collect();
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&recovered_connections)).await
            .map_err(|_| "both recovered API DNS scopes must close all sockets while client clones remain alive")?;
        Ok((busy_error, cancelled, deadline_error, elapsed, recovery_cancelled))
    }).with_subscriber(capture.dispatch()).await;
    operations.abort_all();
    while operations.join_next().await.is_some() {}
    let dns_report = dns.close().await;
    let received = tls.close().await;

    let (busy_error, cancelled, deadline_error, elapsed, recovery_cancelled) = result
        .expect("the owned API DNS lifecycle must finish within its guard")
        .expect("the owned API DNS barriers and admission recovery must complete");
    assert!(
        binding == original,
        "API DNS cancellation and expiration must preserve the immutable bound identity"
    );
    assert!(
        busy_error == Some(Error::Busy),
        "two stalled API DNS operations must occupy both clone-shared provider permits"
    );
    assert!(
        cancelled && recovery_cancelled == 2,
        "all three caller-cancelled API tasks must be joined"
    );
    assert!(
        deadline_error == Some(Error::Deadline),
        "API DNS must reach its connection-phase deadline before any possible POST dispatch"
    );
    assert!(
        (Duration::from_millis(1_800)..Duration::from_millis(3_500)).contains(&elapsed),
        "stalled API DNS must consume the two-second connection budget rather than the twelve-second observation budget"
    );
    assert!(
        dns_report.joined && !dns_report.forced && !dns_report.failed,
        "every API DNS fixture handler must retire and join without forced cleanup"
    );
    assert_eq!(dns_report.questions.len(), 8);
    let (rounds, remainder) = dns_report.questions.as_chunks::<2>();
    assert!(remainder.is_empty());
    for round in rounds {
        address_round(round).expect("every API operation must send only one exact A/AAAA pair");
    }
    assert_eq!(dns_report.answered, 0);
    assert!((4..=8).contains(&dns_report.accepted));
    assert_eq!(
        dns_report.peer_closed.len(),
        dns_report.accepted,
        "no API DNS connection may survive a cancelled or expired operation"
    );
    assert!(
        received.is_empty(),
        "held API DNS and rejected admission must produce no API, mutation, unrestriction or media HTTP"
    );
    let logs = capture.text();
    let events: Vec<serde_json::Value> = logs
        .lines()
        .map(|line| serde_json::from_str(line).expect("owned API DNS logs must remain JSON"))
        .collect();
    for outcome in ["busy", "deadline"] {
        assert_eq!(
            events
                .iter()
                .filter(
                    |event| event["fields"]["event"] == "real_debrid_observation_finished"
                        && event["fields"]["outcome"] == outcome
                )
                .count(),
            1,
            "API admission and connection expiration must each have one controlled workflow outcome"
        );
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| event["fields"]["event"] == "real_debrid_api_dns_finished")
            .count(),
        4,
        "every cancelled or expired API DNS scope must log retirement under its active operation"
    );
    for required in ["request_id", "operation", "outcome", "elapsed_ms"] {
        assert!(logs.contains(required));
    }
    assert!(!logs.contains("unrestriction_uncertain"));
    let safe_errors = [busy_error, deadline_error]
        .into_iter()
        .flatten()
        .map(|error| format!("{error:?} {error}"))
        .collect::<Vec<_>>()
        .join(" ");
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
        MEDIA_TARGET,
        API_HOST,
        "private-",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private) && !safe_errors.contains(private),
            "API DNS logs and errors must exclude credentials, bound identity, private hoster values and destinations"
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
        async move { client.observe(&binding).await.map(drop) }.with_subscriber(capture.dispatch()),
    )
}
