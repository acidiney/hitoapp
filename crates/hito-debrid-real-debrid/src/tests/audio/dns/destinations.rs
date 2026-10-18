use super::super::{
    CALLER_TOKEN, EXPECTED_FORM, FILE, HASH, HOSTER_LINK, MEDIA_TARGET, SERVICE_TOKEN, TITLE,
    TOKEN, TORRENT, WAV_BYTES,
    tls::{API_HOST, Origin},
};
use super::{
    address_round, api_replies,
    destination_tls::{Fixture, Rejection, WRONG_HOST},
    fixture::DnsFixture,
};
use crate::tests::logging::Capture;
use crate::{Error, RealDebridClient, TorrentFileBinding, TorrentPhase};
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn api_mixed_dns_and_invalid_certificates_reject_before_authenticated_http_and_allow_verified_clone_recovery()
 {
    let public_v4 = Ipv4Addr::new(93, 184, 216, 34);
    let public_v6 = Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111);
    for (v4, v6, certificate) in [
        (public_v4, Ipv6Addr::LOCALHOST, None),
        (Ipv4Addr::LOCALHOST, public_v6, None),
        (public_v4, public_v6, Some(Rejection::UntrustedRoot)),
        (public_v4, public_v6, Some(Rejection::WrongName)),
    ] {
        let metadata = api_replies()
            .into_iter()
            .next()
            .expect("the bound completed-torrent metadata must exist");
        let tls = Fixture::start(certificate, metadata).await;
        let dns = DnsFixture::api_rounds(vec![Some((v4, v6)), Some((public_v4, public_v6))]).await;
        let binding = TorrentFileBinding {
            torrent_id: TORRENT.into(),
            info_hash: HASH.into(),
            file_id: 7,
            file_path: FILE.into(),
            file_bytes: WAV_BYTES as u64,
        };
        let original = binding.clone();
        let capture = Capture::default();
        let result = tokio::time::timeout(Duration::from_secs(6), async {
            let client = RealDebridClient::with_test_api_dns(
                TOKEN.into(), tls.ca_der(), tls.address, dns.address,
            ).map_err(|_| "the exact API DNS and TLS fixture client must initialize")?;
            let rejected = client.observe(&binding).await.err();
            let rejected_questions = dns.questions(2).await;
            let rejected_connections = address_round(&rejected_questions[..2])?;
            tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&rejected_connections)).await
                .map_err(|_| "a rejected API destination must retire all DNS sockets before recovery")?;
            if let Some(certificate) = certificate {
                tls.rejected(certificate).await?;
            }
            let before_recovery = tls.counts();
            let clone = client.clone();
            let recovered = clone.observe(&binding).await
                .map_err(|_| "the same client's clone must recover admission through verified API DNS and TLS")?;
            let questions = dns.questions(4).await;
            let recovered_connections = address_round(&questions[2..4])?;
            tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&recovered_connections)).await
                .map_err(|_| "verified clone recovery must retire its DNS sockets while both clients remain alive")?;
            Ok::<_, &'static str>((rejected, before_recovery, recovered))
        }).with_subscriber(capture.dispatch()).await;
        let dns_report = dns.close().await;
        let tls_report = tls.close().await;

        let (rejected, before_recovery, recovered) = result
            .expect("the owned API destination rejection must finish within its guard")
            .expect("API rejection and verified same-client clone recovery must complete");
        assert!(
            rejected == Some(Error::Transport),
            "API destination and certificate failures must have a definite static transport error"
        );
        assert!(
            binding == original && recovered.binding == original,
            "rejection and verified recovery must preserve exact bound identity"
        );
        assert_eq!(recovered.phase, TorrentPhase::ProviderCompleted);
        assert_eq!(recovered.progress_basis_points, Some(10_000));
        assert_eq!(
            before_recovery,
            (usize::from(certificate.is_some()), 0),
            "a mixed answer must make no API connection and invalid TLS must send no authenticated HTTP"
        );
        assert!(dns_report.joined && !dns_report.failed && !dns_report.forced);
        assert_eq!(dns_report.questions.len(), 4);
        let (rounds, remainder) = dns_report.questions.as_chunks::<2>();
        assert!(remainder.is_empty());
        for round in rounds {
            address_round(round)
                .expect("each rejection and recovery must issue only one A/AAAA pair");
        }
        assert_eq!(dns_report.answered, 4);
        assert!((2..=4).contains(&dns_report.accepted));
        assert_eq!(dns_report.peer_closed.len(), dns_report.accepted);
        assert!(
            tls_report.joined && !tls_report.failed && !tls_report.forced,
            "every expected rejection and valid TLS handler must complete and join without forced cleanup"
        );
        assert_eq!(tls_report.accepted, 1 + usize::from(certificate.is_some()));
        assert_eq!(tls_report.completed, tls_report.accepted);
        assert!(
            tls_report.rejected == certificate.into_iter().collect::<Vec<_>>(),
            "only the explicitly configured certificate rejection may be accepted by the fixture"
        );
        assert_eq!(
            tls_report.received.len(),
            1,
            "only the later verified manual observation may send HTTP, with no retry, POST or media request"
        );
        let observed = &tls_report.received[0];
        assert!(observed.origin == Origin::Api && observed.server_name == API_HOST);
        let request = &observed.request;
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
        let logs = capture.text();
        let events: Vec<serde_json::Value> = logs
            .lines()
            .map(|line| {
                serde_json::from_str(line).expect("owned API destination logs must remain JSON")
            })
            .collect();
        for outcome in ["transport", "success"] {
            assert_eq!(
                events
                    .iter()
                    .filter(
                        |event| event["fields"]["event"] == "real_debrid_observation_finished"
                            && event["fields"]["outcome"] == outcome
                    )
                    .count(),
                1
            );
        }
        assert_eq!(
            events
                .iter()
                .filter(
                    |event| event["fields"]["event"] == "real_debrid_api_dns_finished"
                        && event["fields"]["outcome"] == "complete"
                )
                .count(),
            2
        );
        assert!(!logs.contains("unrestriction_uncertain"));
        for required in ["request_id", "operation", "outcome", "elapsed_ms"] {
            assert!(logs.contains(required));
        }
        let safe_error = rejected
            .map(|error| format!("{error:?} {error}"))
            .unwrap_or_default();
        let v4 = v4.to_string();
        let v6 = v6.to_string();
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
            WRONG_HOST,
            v4.as_str(),
            v6.as_str(),
            "private-",
            "http://",
            "https://",
        ] {
            assert!(
                !logs.contains(private) && !safe_error.contains(private),
                "API destination errors and active scoped logs must exclude all credentials, private identity, hoster values and DNS/certificate names"
            );
        }
    }
}
