use super::super::{
    CALLER_TOKEN, EXPECTED_FORM, FILE, HASH, HOSTER_LINK, MEDIA_TARGET, SERVICE_TOKEN, TITLE,
    TOKEN, TORRENT, WAV_BYTES,
    tls::{API_HOST, Fixture, Origin},
    wave,
};
use super::{address_round, api_replies, fixture::DnsFixture};
use crate::tests::logging::Capture;
use crate::{Error, RealDebridClient, TorrentFileBinding, TorrentPhase};
use std::{
    collections::BTreeSet,
    net::{Ipv4Addr, Ipv6Addr},
    time::{Duration, Instant},
};
use tokio::task::JoinSet;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn public_api_dns_opens_bound_tls_and_a_stalled_post_preflight_is_definite_before_manual_recovery()
 {
    let public = (
        Ipv4Addr::new(93, 184, 216, 34),
        Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111),
    );
    let metadata = api_replies()
        .into_iter()
        .next()
        .expect("the owned completed-torrent reply must be present");
    let tls = Fixture::start(vec![metadata.clone(), metadata.clone(), metadata], wave()).await;
    let dns = DnsFixture::api_rounds(vec![Some(public), Some(public), None, Some(public)]).await;
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
        let client = RealDebridClient::with_test_api_dns(
            TOKEN.into(), tls.ca_der(), tls.api_address, dns.address,
        ).map_err(|_| "the fixed API DNS and TLS client must initialize")?;
        let observed = client.observe(&binding).await
            .map_err(|_| "public A and AAAA answers must admit the exact bound API observation")?;
        let first_questions = dns.questions(2).await;
        let first_connections = address_round(&first_questions[..2])?;
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&first_connections)).await
            .map_err(|_| "successful API resolution must retire its query sockets before further work")?;
        let clone = client.clone();
        let bound = binding.clone();
        let started = Instant::now();
        operations.spawn(
            async move { clone.unrestrict_bound_file(&bound).await }
                .with_subscriber(capture.dispatch()),
        );
        let preflight_questions = dns.questions(6).await;
        let get_connections = address_round(&preflight_questions[2..4])?;
        let post_connections = address_round(&preflight_questions[4..6])?;
        if !get_connections.is_disjoint(&post_connections) {
            return Err("GET and POST preflights must own independent DNS connections");
        }
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&get_connections)).await
            .map_err(|_| "the acknowledged GET must retire its DNS sockets while the POST lookup is held")?;
        let unrestriction = operations.join_next().await
            .ok_or("the held POST preflight operation must remain owned")?
            .map_err(|_| "the POST preflight must return its typed deadline without task failure")?;
        let deadline_error = unrestriction.err();
        let elapsed = started.elapsed();
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&post_connections)).await
            .map_err(|_| "a definite POST preflight deadline must close all query sockets while the same client remains alive")?;
        let recovered = client.observe(&binding).await
            .map_err(|_| "a later explicit observation must reuse admission after the definite preflight failure")?;
        let questions = dns.questions(8).await;
        let recovered_connections = address_round(&questions[6..8])?;
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&recovered_connections)).await
            .map_err(|_| "the manual recovery must retire its DNS sockets before the live client is released")?;
        Ok::<_, &'static str>((observed, deadline_error, elapsed, recovered))
    }).with_subscriber(capture.dispatch()).await;
    operations.abort_all();
    while operations.join_next().await.is_some() {}
    let dns_report = dns.close().await;
    let received = tls.close().await;

    let (observed, deadline_error, elapsed, recovered) = result
        .expect("the owned API preflight workflow must finish within its guard")
        .expect("the owned API preflight barriers and manual recovery must succeed");
    assert!(
        binding == original && observed.binding == original && recovered.binding == original,
        "successful DNS, definite POST preflight failure and manual recovery must preserve the exact bound identity"
    );
    for observation in [observed, recovered] {
        assert_eq!(observation.phase, TorrentPhase::ProviderCompleted);
        assert_eq!(observation.progress_basis_points, Some(10_000));
    }
    assert!(
        deadline_error == Some(Error::Deadline),
        "a held DNS lookup before POST dispatch must be a definite connection deadline rather than an uncertain acknowledgement"
    );
    assert!(
        (Duration::from_millis(1_800)..Duration::from_millis(4_000)).contains(&elapsed),
        "the completed GET plus held POST DNS must use the two-second connection phase and bounded account pacing rather than the twelve-second operation budget"
    );
    assert!(
        dns_report.joined && !dns_report.forced && !dns_report.failed,
        "all scripted API DNS handlers must retire and join without forced cleanup"
    );
    assert_eq!(dns_report.questions.len(), 8);
    let (rounds, remainder) = dns_report.questions.as_chunks::<2>();
    assert!(remainder.is_empty());
    for round in rounds {
        address_round(round).expect("every API preflight must issue only its exact A/AAAA pair");
    }
    assert_eq!(
        dns_report.answered, 6,
        "three public-answer rounds must complete and the one POST preflight round must remain unanswered without retry"
    );
    assert!((4..=8).contains(&dns_report.accepted));
    let connections: BTreeSet<_> = dns_report
        .questions
        .iter()
        .map(|question| question.connection)
        .collect();
    assert!(connections == dns_report.peer_closed);
    assert_eq!(dns_report.peer_closed.len(), dns_report.accepted);
    assert_eq!(
        received.len(),
        3,
        "only the initial observation, acknowledged GET and later explicit observation may reach HTTP; the held preflight must send no POST or media request"
    );
    for observed in received {
        assert!(observed.origin == Origin::Api && observed.server_name == API_HOST);
        let request = observed.request;
        assert!(
            request.method == "GET"
                && request.target == format!("/rest/1.0/torrents/info/{TORRENT}")
        );
        assert!(request.header("host") == Some(API_HOST));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert_eq!(request.header("accept"), Some("application/json"));
        assert_eq!(request.header("accept-encoding"), Some("identity"));
        assert!(request.body.is_empty());
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
        .map(|line| serde_json::from_str(line).expect("owned API preflight logs must remain JSON"))
        .collect();
    for (event, outcome, count) in [
        ("real_debrid_observation_finished", "success", 2),
        ("real_debrid_unrestriction_finished", "deadline", 1),
        ("real_debrid_api_dns_finished", "complete", 3),
    ] {
        assert_eq!(
            events
                .iter()
                .filter(|entry| entry["fields"]["event"] == event
                    && entry["fields"]["outcome"] == outcome)
                .count(),
            count
        );
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| event["fields"]["event"] == "real_debrid_api_dns_finished")
            .count(),
        4
    );
    assert!(!logs.contains("unrestriction_uncertain"));
    for required in ["request_id", "operation", "outcome", "elapsed_ms"] {
        assert!(logs.contains(required));
    }
    let safe_error = deadline_error
        .map(|error| format!("{error:?} {error}"))
        .unwrap_or_default();
    let v4 = public.0.to_string();
    let v6 = public.1.to_string();
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
        v4.as_str(),
        v6.as_str(),
        "private-",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private) && !safe_error.contains(private),
            "API preflight logs and errors must exclude credentials, bound identity, hoster values and DNS destinations"
        );
    }
}
