use super::super::{
    CALLER_TOKEN, EXPECTED_FORM, FILE, HASH, HOSTER_LINK, MEDIA_TARGET, SERVICE_TOKEN, TITLE,
    TOKEN, TORRENT, WAV_BYTES,
    tls::{API_HOST, DNS_MEDIA_HOST as MEDIA_HOST, Fixture, Origin, Received},
    wave,
};
use super::{address_round, api_replies, fixture::DnsFixture};
use crate::tests::logging::Capture;
use crate::{Error, MediaAllowlist, OpenedBoundAudio, RealDebridClient, TorrentFileBinding};
use std::{
    collections::BTreeSet,
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn mixed_public_and_private_dns_answers_reject_before_media_and_restore_both_body_slots() {
    let public_v4 = Ipv4Addr::new(93, 184, 216, 34);
    let public_v6 = Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111);
    for (v4, v6) in [
        (public_v4, Ipv6Addr::LOCALHOST),
        (Ipv4Addr::LOCALHOST, public_v6),
    ] {
        let wave = wave();
        let tls =
            Fixture::with_dns_media(api_replies().into_iter().take(6).collect(), wave.clone())
                .await;
        let dns = DnsFixture::answering(v4, v6).await;
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
            let client = RealDebridClient::with_test_tls_dns(
                TOKEN.into(), tls.ca_der(), tls.api_address, (MEDIA_HOST, tls.media_address), dns.address,
            ).map_err(|_| "the fixed-host mixed-answer client must initialize")?;
            let allowlist = MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")])
                .map_err(|_| "the exact mixed-answer media allowlist must initialize")?;
            let rejection = client.open_bound_audio(&binding, &allowlist).await.err();
            if rejection != Some(Error::MediaDestination) { return Ok((rejection, None)); }
            let rejected_questions = dns.questions(2).await;
            let rejected_connections = address_round(&rejected_questions[..2])?;
            tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&rejected_connections)).await
                .map_err(|_| "rejected mixed answers must retire every DNS connection before reuse")?;
            dns.set_answers(public_v4, public_v6);
            let mut first = client.open_bound_audio(&binding, &allowlist).await
                .map_err(|_| "the same client must recover its first media slot after rejection")?;
            let clone = client.clone();
            let mut second = clone.open_bound_audio(&binding, &allowlist).await
                .map_err(|_| "a clone must recover the other media slot while the first body stays owned")?;
            let first_binding = first.binding.clone();
            let second_binding = second.binding.clone();
            let first_bytes = consume(&mut first).await?;
            let second_bytes = consume(&mut second).await?;
            let questions = dns.questions(6).await;
            for round in questions[..6].as_chunks::<2>().0 { address_round(round)?; }
            let connections: BTreeSet<_> = questions.iter().map(|question| question.connection).collect();
            tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&connections)).await
                .map_err(|_| "all rejected and recovered DNS connections must retire while client clones remain alive")?;
            Ok::<_, &'static str>((rejection, Some((first_binding, second_binding, first_bytes, second_bytes))))
        }).with_subscriber(capture.dispatch()).await;
        let dns_report = dns.close().await;
        let received = tls.close().await;

        let (rejection, recovery) = result
            .expect("the owned mixed-answer workflow must finish within its guard")
            .expect("the owned mixed-answer barriers and recovery must succeed");
        assert!(
            rejection == Some(Error::MediaDestination),
            "a private answer in either address family must reject the complete destination set after a known acknowledgement"
        );
        let (first_binding, second_binding, first_bytes, second_bytes) =
            recovery.expect("both media permits must be reusable after the mixed-answer rejection");
        assert!(
            binding == original && first_binding == original && second_binding == original,
            "rejection and full media admission recovery must preserve the exact bound identity"
        );
        assert!(
            first_bytes == wave && second_bytes == wave,
            "both concurrently owned recovered bodies must deliver the unchanged exact WAV through EOF"
        );
        assert!(
            dns_report.joined && !dns_report.forced && !dns_report.failed,
            "all owned mixed-answer DNS handlers must retire and join without forced cleanup"
        );
        assert_eq!(dns_report.questions.len(), 6);
        let (_, remainder) = dns_report.questions.as_chunks::<2>();
        assert!(
            remainder.is_empty(),
            "owned DNS questions must form complete A/AAAA pairs"
        );
        assert_eq!(
            dns_report.answered, 6,
            "three admitted operations may send only one answered A/AAAA pair each without retries"
        );
        assert!((3..=6).contains(&dns_report.accepted));
        assert_eq!(dns_report.peer_closed.len(), dns_report.accepted);
        assert_requests(&received);
        let logs = capture.text();
        let events: Vec<serde_json::Value> = logs
            .lines()
            .map(|line| {
                serde_json::from_str(line).expect("the owned mixed-answer logs must remain JSON")
            })
            .collect();
        assert_eq!(
            events
                .iter()
                .filter(
                    |event| event["fields"]["event"] == "real_debrid_audio_opening_finished"
                        && event["fields"]["outcome"] == "media_destination"
                )
                .count(),
            1,
            "each mixed-answer operation must log one controlled destination rejection"
        );
        for (event, expected) in [
            ("remote_audio_media_dns_finished", 3),
            ("real_debrid_audio_opening_finished", 2),
            ("remote_audio_body_finished", 2),
        ] {
            assert_eq!(
                events
                    .iter()
                    .filter(|entry| entry["fields"]["event"] == event
                        && entry["fields"]["outcome"] == "complete")
                    .count(),
                expected
            );
        }
        for field in ["request_id", "operation", "outcome", "elapsed_ms"] {
            assert!(logs.contains(field));
        }
        assert!(
            !logs.contains("unrestriction_uncertain"),
            "known acknowledgement destination rejection must never be classified as uncertain"
        );
        let safe_error = rejection
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
            MEDIA_HOST,
            v4.as_str(),
            v6.as_str(),
            "private-",
            "http://",
            "https://",
        ] {
            assert!(
                !logs.contains(private) && !safe_error.contains(private),
                "mixed-answer logs and errors must exclude all private credentials, identities, URLs and DNS values"
            );
        }
    }
}

async fn consume(audio: &mut OpenedBoundAudio) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Vec::new();
    loop {
        let chunk = audio
            .next_chunk()
            .await
            .map_err(|_| "a recovered owned body must remain readable")?;
        let Some(chunk) = chunk else {
            return Ok(bytes);
        };
        if chunk.is_empty()
            || chunk.len() > 65_536
            || chunk.len() > WAV_BYTES.saturating_sub(bytes.len())
        {
            return Err("a recovered body exceeded its exact pull or representation bound");
        }
        bytes.extend_from_slice(&chunk);
    }
}

fn assert_requests(received: &[Received]) {
    assert_eq!(
        received.len(),
        8,
        "mixed-answer rejection must produce zero media HTTP, followed by exactly two successful bound audio openings"
    );
    for (index, method, target) in [
        (0, "GET", format!("/rest/1.0/torrents/info/{TORRENT}")),
        (1, "POST", "/rest/1.0/unrestrict/link".into()),
        (2, "GET", format!("/rest/1.0/torrents/info/{TORRENT}")),
        (3, "POST", "/rest/1.0/unrestrict/link".into()),
        (5, "GET", format!("/rest/1.0/torrents/info/{TORRENT}")),
        (6, "POST", "/rest/1.0/unrestrict/link".into()),
    ] {
        let observed = &received[index];
        assert!(observed.origin == Origin::Api && observed.server_name == API_HOST);
        let request = &observed.request;
        assert!(request.method == method && request.target == target);
        assert!(request.header("host") == Some(API_HOST));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert_eq!(request.header("accept-encoding"), Some("identity"));
        assert!(if method == "GET" {
            request.body.is_empty()
        } else {
            request.body.as_slice() == EXPECTED_FORM.as_bytes()
        });
        if method == "POST" {
            assert_eq!(
                request.header("content-type"),
                Some("application/x-www-form-urlencoded")
            );
        }
    }
    for index in [4, 7] {
        let media = &received[index];
        assert!(media.origin == Origin::Media && media.server_name == MEDIA_HOST);
        assert!(media.request.method == "GET" && media.request.target == MEDIA_TARGET);
        assert!(media.request.header("host") == Some(MEDIA_HOST));
        assert!(media.request.header("authorization").is_none());
        assert_eq!(media.request.header("accept-encoding"), Some("identity"));
        assert!(media.request.body.is_empty());
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
