#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

use hito_plugin_sdk::{Capability, Manifest, ProviderRecording, RecordingRequest, SourcePage};
use reqwest::{Client, Response, StatusCode, Url, header::HeaderMap};
use serde::de::DeserializeOwned;
use std::{
    env, fs,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

const MAX_ENCODED_BYTES: u64 = 128 * 1024 * 1024;

#[tokio::test(flavor = "current_thread")]
#[ignore = "Requires an explicitly opted-in isolated release image and public recording"]
async fn release_image_relays_public_aac_with_ranges_and_native_decoding() {
    assert!(
        env::var("HITO_YOUTUBE_LIVE").ok().as_deref() == Some("1"),
        "Set the explicit live-image opt-in before running this ignored test"
    );
    let base = base_url();
    let video = env::var("HITO_YOUTUBE_LIVE_VIDEO_ID")
        .unwrap_or_else(|_| panic!("The live test requires an explicit public video ID"));
    assert!(
        video.len() == 11
            && video
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)),
        "The live test video ID must use eleven canonical ASCII characters"
    );
    let mut encoded = tempfile::NamedTempFile::new()
        .unwrap_or_else(|_| panic!("The private encoded-audio file could not be created"));
    encoded
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|_| panic!("The encoded-audio file could not be made private"));
    let permissions = encoded
        .as_file()
        .metadata()
        .unwrap_or_else(|_| panic!("The private encoded-audio file could not be checked"))
        .permissions();
    assert_eq!(permissions.mode() & 0o777, 0o600);
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .http1_only()
        .pool_max_idle_per_host(0)
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| panic!("The isolated live-image HTTP client could not be created"));
    let manifest: Manifest = json(client.get(endpoint(&base, "v1/manifest"))).await;
    manifest
        .validate()
        .unwrap_or_else(|_| panic!("The release image returned an invalid manifest"));
    assert!(manifest.capabilities.contains(&Capability::Search));
    assert!(manifest.capabilities.contains(&Capability::Sources));
    assert!(manifest.authorization.is_none());
    let request = RecordingRequest {
        recording_id: video,
    };
    let detail: ProviderRecording = json(
        client
            .post(endpoint(&base, "v1/recordings/detail"))
            .json(&request),
    )
    .await;
    detail
        .validate()
        .unwrap_or_else(|_| panic!("The release image returned invalid recording metadata"));
    assert!(
        detail.id == request.recording_id,
        "The recording identity changed"
    );
    let page = sources(&client, &base, &request).await;
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].ready);
    let source = &page.items[0].id;
    assert!(source.starts_with("youtube:v1:"));
    let mut audio = endpoint(&base, "v1/audio");
    audio.query_pairs_mut().append_pair("source_id", source);
    let started = Instant::now();
    let head = send(client.head(audio.clone()).timeout(Duration::from_secs(10))).await;
    let total = audio_headers(&head, StatusCode::OK);
    assert!((64..=MAX_ENCODED_BYTES).contains(&total));
    assert!(!head.headers().contains_key("content-range"));
    assert!(bytes(head, 0).await.is_empty());
    let partial = send(
        client
            .get(audio.clone())
            .header("range", "bytes=16-47")
            .timeout(Duration::from_secs(10)),
    )
    .await;
    assert_eq!(audio_headers(&partial, StatusCode::PARTIAL_CONTENT), 32);
    assert!(header(partial.headers(), "content-range") == format!("bytes 16-47/{total}"));
    let selected = bytes(partial, 32).await;
    assert_eq!(selected.len(), 32);
    let mut full = send(client.get(audio)).await;
    assert_eq!(audio_headers(&full, StatusCode::OK), total);
    assert!(!full.headers().contains_key("content-range"));
    let transfer_deadline = Instant::now() + Duration::from_secs(60);
    let mut received = 0_u64;
    while let Some(chunk) = full
        .chunk()
        .await
        .unwrap_or_else(|_| panic!("The full encoded-audio transfer failed"))
    {
        assert!(
            Instant::now() < transfer_deadline,
            "The audio transfer exceeded its deadline"
        );
        received += chunk.len() as u64;
        assert!(received <= total && received <= MAX_ENCODED_BYTES);
        encoded
            .as_file_mut()
            .write_all(&chunk)
            .unwrap_or_else(|_| panic!("The private encoded-audio file could not be written"));
    }
    assert_eq!(received, total);
    encoded
        .as_file_mut()
        .flush()
        .unwrap_or_else(|_| panic!("The private encoded-audio file could not be flushed"));
    encoded
        .as_file_mut()
        .seek(SeekFrom::Start(16))
        .unwrap_or_else(|_| panic!("The private encoded-audio file could not be read"));
    let mut from_full = [0; 32];
    encoded
        .as_file_mut()
        .read_exact(&mut from_full)
        .unwrap_or_else(|_| panic!("The full transfer did not contain the selected byte range"));
    assert!(
        selected.as_slice() == from_full.as_slice(),
        "The range and full transfer represent different bytes"
    );
    let http_ms = started.elapsed().as_millis() as u64;
    let repeated = sources(&client, &base, &request).await;
    assert_eq!(repeated.items.len(), 1);
    assert!(
        repeated.items[0].id.as_str() == source.as_str(),
        "The source identity changed during playback"
    );

    let decode_started = Instant::now();
    let decode_deadline = decode_started + Duration::from_secs(15);
    let input = encoded
        .reopen()
        .unwrap_or_else(|_| panic!("The private encoded-audio file could not be reopened"));
    let mut decoder = hito_audio::Decoder::open(input)
        .unwrap_or_else(|_| panic!("The native decoder could not open the delivered AAC audio"));
    let format = decoder.format();
    assert!((8_000..=192_000).contains(&format.sample_rate));
    assert!((1..=8).contains(&format.channels));
    let maximum_samples = u64::from(format.sample_rate) * u64::from(format.channels) * 3_600;
    let mut samples = 0_u64;
    let mut energy = 0_f64;
    while let Some(block) = decoder
        .read_until(|| Instant::now() >= decode_deadline)
        .unwrap_or_else(|_| panic!("The native AAC decoder failed or exceeded its deadline"))
    {
        assert!(block.samples.iter().all(|sample| sample.is_finite()));
        samples += block.samples.len() as u64;
        assert!(
            samples <= maximum_samples,
            "Decoded audio exceeded its duration bound"
        );
        energy += block
            .samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>();
    }
    assert!(Instant::now() < decode_deadline);
    assert!(samples >= u64::from(format.sample_rate) * u64::from(format.channels) / 10);
    let mean_square = energy / samples as f64;
    assert!(mean_square.is_finite() && mean_square > 0.);
    eprintln!(
        "{}",
        serde_json::json!({
            "event":"youtube_live_audio_verified", "encoded_bytes":received,
            "sample_rate":format.sample_rate, "channels":format.channels,
            "decoded_samples":samples, "mean_square":mean_square,
            "http_ms":http_ms, "decode_ms":decode_started.elapsed().as_millis() as u64
        })
    );
}

fn base_url() -> Url {
    let raw = env::var("HITO_YOUTUBE_LIVE_BASE_URL")
        .unwrap_or_else(|_| panic!("The live test requires an explicit isolated base URL"));
    let url = Url::parse(&raw).unwrap_or_else(|_| panic!("The isolated base URL is invalid"));
    let local = match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        Some(url::Host::Domain(host)) => matches!(
            host,
            "plugin-youtube" | "plugin-youtube-live" | "hito-youtube-live"
        ),
        None => false,
    };
    assert!(
        local
            && url.scheme() == "http"
            && url.port().is_some_and(|port| port != 0)
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "The live test requires a literal loopback or approved isolated-service HTTP endpoint"
    );
    url
}

fn endpoint(base: &Url, path: &str) -> Url {
    base.join(path)
        .unwrap_or_else(|_| panic!("The isolated plugin API path is invalid"))
}

async fn send(request: reqwest::RequestBuilder) -> Response {
    request
        .send()
        .await
        .unwrap_or_else(|_| panic!("The isolated plugin HTTP request failed"))
}

async fn bytes(mut response: Response, limit: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .unwrap_or_else(|_| panic!("The isolated plugin response body failed"))
    {
        assert!(
            chunk.len() <= limit.saturating_sub(bytes.len()),
            "A plugin response exceeded its byte bound"
        );
        bytes.extend_from_slice(&chunk);
    }
    bytes
}

async fn json<T: DeserializeOwned>(request: reqwest::RequestBuilder) -> T {
    let response = send(request.timeout(Duration::from_secs(10))).await;
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&bytes(response, 65_536).await)
        .unwrap_or_else(|_| panic!("The isolated plugin returned invalid SDK JSON"))
}

async fn sources(client: &Client, base: &Url, request: &RecordingRequest) -> SourcePage {
    let page: SourcePage = json(client.post(endpoint(base, "v1/sources")).json(request)).await;
    page.validate()
        .unwrap_or_else(|_| panic!("The release image returned invalid sources"));
    let wire = serde_json::to_vec(&page)
        .unwrap_or_else(|_| panic!("The returned sources could not be checked"));
    assert!(
        !wire.windows(3).any(|part| part == b"://"),
        "Sources exposed a URL"
    );
    page
}

fn audio_headers(response: &Response, status: StatusCode) -> u64 {
    assert_eq!(response.status(), status);
    let headers = response.headers();
    assert!(header(headers, "content-type") == "audio/mp4");
    assert!(header(headers, "accept-ranges") == "bytes");
    assert!(header(headers, "cache-control") == "private, no-store");
    for forbidden in [
        "location",
        "set-cookie",
        "content-encoding",
        "transfer-encoding",
    ] {
        assert!(
            !headers.contains_key(forbidden),
            "Audio returned a forbidden transport header"
        );
    }
    header(headers, "content-length")
        .parse()
        .unwrap_or_else(|_| panic!("Audio returned an invalid declared length"))
}

fn header<'a>(headers: &'a HeaderMap, name: &'static str) -> &'a str {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_else(|| panic!("Audio omitted a required valid transport header"))
}
