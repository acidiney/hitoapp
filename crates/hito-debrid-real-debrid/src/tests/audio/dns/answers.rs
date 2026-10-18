use super::super::{
    CALLER_TOKEN, CHANNELS, EXPECTED_FORM, FILE, FRAMES, HASH, HOSTER_LINK, MEDIA_TARGET,
    SAMPLE_RATE, SERVICE_TOKEN, TITLE, TOKEN, TORRENT, WAV_BYTES, pcm_sample,
    tls::{API_HOST, DNS_MEDIA_HOST as MEDIA_HOST, Fixture, Origin, Received},
    wave,
};
use super::{address_round, api_replies, fixture::DnsFixture};
use crate::tests::logging::Capture;
use crate::{MediaAllowlist, RealDebridClient, TorrentFileBinding};
use hito_audio::Decoder;
use std::{
    io::Cursor,
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn public_dual_stack_dns_answers_open_and_decode_the_exact_bound_audio_without_exposing_private_data()
 {
    let wave = wave();
    let tls =
        Fixture::with_dns_media(api_replies().into_iter().take(2).collect(), wave.clone()).await;
    let public_v4 = Ipv4Addr::new(93, 184, 216, 34);
    let public_v6 = Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111);
    let dns = DnsFixture::answering(public_v4, public_v6).await;
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: WAV_BYTES as u64,
    };
    let original = binding.clone();
    let capture = Capture::default();
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let client = RealDebridClient::with_test_tls_dns(
            TOKEN.into(), tls.ca_der(), tls.api_address, (MEDIA_HOST, tls.media_address), dns.address,
        ).map_err(|_| "the fixed-host owned answer client must initialize")?;
        let allowlist = MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")])
            .map_err(|_| "the exact owned DNS media allowlist must initialize")?;
        let mut audio = client.open_bound_audio(&binding, &allowlist).await
            .map_err(|_| "valid public dual-stack answers must permit canonical bound audio opening")?;
        let returned_binding = audio.binding.clone();
        let length = audio.length;
        let format = (audio.format.sample_rate, audio.format.channels, audio.format.frames);
        let mut bytes = Vec::new();
        loop {
            let chunk = audio.next_chunk().await.map_err(|_| "the DNS-qualified owned WAV must remain readable through EOF")?;
            let Some(chunk) = chunk else { break; };
            if chunk.is_empty() || chunk.len() > 65_536 || chunk.len() > WAV_BYTES.saturating_sub(bytes.len()) {
                return Err("the DNS-qualified body exceeded its exact representation bound");
            }
            bytes.extend_from_slice(&chunk);
        }
        let questions = dns.questions(2).await;
        let connections = address_round(&questions[..2])?;
        tokio::time::timeout(Duration::from_secs(1), dns.peer_closed(&connections)).await
            .map_err(|_| "successful resolution must retire its DNS connections while the client remains alive")?;
        Ok((returned_binding, length, format, bytes))
    }).with_subscriber(capture.dispatch()).await;
    let dns_report = dns.close().await;
    let received = tls.close().await;

    let (returned_binding, length, format, bytes) = result
        .expect("the complete owned DNS answer workflow must finish within its guard")
        .expect("the owned DNS answer workflow must succeed");
    assert!(
        binding == original && returned_binding == original,
        "successful DNS resolution must preserve the exact immutable torrent-file binding"
    );
    assert_eq!(length, WAV_BYTES as u64);
    assert_eq!(format, (SAMPLE_RATE, CHANNELS, FRAMES));
    assert!(
        bytes == wave,
        "DNS qualification and media delivery must preserve every original WAV byte"
    );
    assert!(
        dns_report.joined && !dns_report.forced && !dns_report.failed,
        "all owned DNS handlers must retire naturally and join"
    );
    assert_eq!(
        dns_report.questions.len(),
        2,
        "only one exact A and one exact AAAA question may be issued"
    );
    assert_eq!(
        dns_report.answered, 2,
        "both public address answers must be transmitted over the owned DNS protocol"
    );
    assert!((1..=2).contains(&dns_report.accepted));
    assert_eq!(
        dns_report.peer_closed.len(),
        dns_report.accepted,
        "successful resolution must leave no owned DNS connection alive"
    );
    assert_requests(&received);
    let mut decoder =
        Decoder::open(Cursor::new(bytes)).expect("the DNS-qualified delivered WAV must decode");
    assert_eq!(decoder.format().sample_rate, SAMPLE_RATE);
    assert_eq!(decoder.format().channels, CHANNELS);
    let mut samples = 0_usize;
    while let Some(block) = decoder
        .read()
        .expect("the DNS-qualified PCM body must decode through EOF")
    {
        for sample in block.samples {
            let expected = f32::from(pcm_sample(samples)) / 32768.0;
            assert!(
                (*sample - expected).abs() < 0.000_001,
                "all decoded samples must retain the exact owned PCM values"
            );
            samples += 1;
        }
    }
    assert_eq!(samples as u64, FRAMES * u64::from(CHANNELS));
    let logs = capture.text();
    let events: Vec<serde_json::Value> = logs
        .lines()
        .map(|line| {
            serde_json::from_str(line).expect("the owned DNS answer workflow logs must remain JSON")
        })
        .collect();
    for event in [
        "real_debrid_media_dns_finished",
        "real_debrid_audio_opening_finished",
        "real_debrid_audio_body_finished",
    ] {
        assert_eq!(
            events
                .iter()
                .filter(|entry| entry["fields"]["event"] == event
                    && entry["fields"]["outcome"] == "complete")
                .count(),
            1,
            "successful resolution, opening and EOF must each have one safe completion event"
        );
    }
    assert!(events.iter().any(|entry| entry["fields"]["event"]
        == "real_debrid_audio_body_finished"
        && entry["fields"]["bytes"].as_u64() == Some(WAV_BYTES as u64)));
    for field in ["request_id", "operation", "outcome", "elapsed_ms"] {
        assert!(logs.contains(field));
    }
    assert!(!logs.contains("unrestriction_uncertain"));
    let download = format!("https://{MEDIA_HOST}{MEDIA_TARGET}");
    let v4 = public_v4.to_string();
    let v6 = public_v6.to_string();
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
        download.as_str(),
        v4.as_str(),
        v6.as_str(),
        "private-",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private),
            "DNS answer workflow logs must exclude all private identity, credential, URL and address values"
        );
    }
}

fn assert_requests(received: &[Received]) {
    assert_eq!(
        received.len(),
        3,
        "successful DNS may produce only one metadata GET, exact unrestriction POST and one direct media GET"
    );
    for (index, method, target) in [
        (0, "GET", format!("/rest/1.0/torrents/info/{TORRENT}")),
        (1, "POST", "/rest/1.0/unrestrict/link".into()),
    ] {
        let observed = &received[index];
        assert!(observed.origin == Origin::Api && observed.server_name == API_HOST);
        let request = &observed.request;
        assert!(request.method == method && request.target == target);
        assert!(request.header("host") == Some(API_HOST));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert_eq!(request.header("accept"), Some("application/json"));
        assert_eq!(request.header("accept-encoding"), Some("identity"));
    }
    assert!(received[0].request.body.is_empty());
    assert!(received[1].request.body.as_slice() == EXPECTED_FORM.as_bytes());
    assert_eq!(
        received[1].request.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert!(
        received[1].request.header("content-length")
            == Some(EXPECTED_FORM.len().to_string().as_str())
    );
    let media = &received[2];
    assert!(
        media.origin == Origin::Media && media.server_name == MEDIA_HOST,
        "the admitted media must retain the exact allowlisted hostname and verified TLS SNI"
    );
    assert!(media.request.method == "GET" && media.request.target == MEDIA_TARGET);
    assert!(media.request.header("host") == Some(MEDIA_HOST));
    assert!(media.request.header("authorization").is_none());
    assert_eq!(media.request.header("accept-encoding"), Some("identity"));
    assert!(media.request.body.is_empty());
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
