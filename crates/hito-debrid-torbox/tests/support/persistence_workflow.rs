use super::{fixtures, provider, scripted_media};
use hito_debrid_torbox::{GeneratedFileCandidate, MediaAllowlist, TorBoxClient};
use provider::reply::Reply;
use tokio::time::Instant;

pub struct Fixture {
    provider: provider::Fixture,
    media: scripted_media::ScriptedMedia,
}

impl Fixture {
    pub async fn start() -> Self {
        let observed = || Reply::json(fixtures::observed(true, true, "album/owned.wav"));
        let provider = provider::Fixture::responses(vec![
            observed(),
            Reply::json(fixtures::acknowledgment()),
            observed(),
            observed(),
        ])
        .await;
        let media = scripted_media::ScriptedMedia::start(vec![
            scripted_media::Reply::full(fixtures::wave()),
            scripted_media::Reply::full(fixtures::wave()),
        ])
        .await;
        Self { provider, media }
    }

    pub fn client(&self) -> TorBoxClient {
        self.client_with_credential(fixtures::TOKEN)
    }

    pub fn client_with_credential(&self, credential: &str) -> TorBoxClient {
        TorBoxClient::with_test_tls_media(
            credential.into(),
            self.provider.ca_der(),
            self.provider.address(),
            self.media.ca_der(),
            self.media.address(),
        )
        .unwrap()
    }

    pub async fn close(self) -> (Vec<provider::Request>, usize) {
        let requests = self.provider.close().await;
        let media_requests = self.media.close().await;
        (requests, media_requests)
    }
}

pub async fn complete(
    client: &TorBoxClient,
    candidate: &GeneratedFileCandidate,
    allowlist: &MediaAllowlist,
) -> (Instant, Instant) {
    let started = Instant::now();
    let mut audio = client
        .admit_bound_audio()
        .unwrap()
        .open_retained(candidate, allowlist)
        .await
        .expect("the restored candidate must freshly qualify its exact owned WAV");
    assert!(audio.binding == *candidate.binding());
    assert_eq!(audio.length, fixtures::WAV_BYTES);
    let hito_remote_audio::AudioFormat::Pcm16Wav(wav_format) = audio.format else {
        panic!("the unchanged WAV policy must return a WAV descriptor");
    };
    assert_eq!(
        (
            wav_format.sample_rate,
            wav_format.channels,
            wav_format.frames
        ),
        (fixtures::SAMPLE_RATE, fixtures::CHANNELS, fixtures::FRAMES)
    );
    let mut received = Vec::new();
    while let Some(chunk) = audio.next_chunk().await.unwrap() {
        assert!(chunk.len() <= (fixtures::WAV_BYTES as usize).saturating_sub(received.len()));
        received.extend_from_slice(&chunk);
    }
    assert!(received == fixtures::wave());
    fixtures::assert_pcm(received);
    (started, Instant::now())
}

pub fn assert_requests(requests: &[provider::Request], open_windows: [(Instant, Instant); 2]) {
    assert_eq!(requests.len(), 4);
    for (index, request) in requests.iter().enumerate() {
        assert!(request.method == "GET" && request.server_name == "api.torbox.app");
        assert!(request.body.is_empty());
        assert!(request.header("host") == Some("api.torbox.app"));
        assert!(request.header("accept") == Some("application/json"));
        assert!(request.header("accept-encoding") == Some("identity"));
        for name in [
            "cookie",
            "referer",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ] {
            assert!(request.header(name).is_none());
        }
        if index == 1 {
            assert!(
                request.target
                    == "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false"
            );
            assert!(request.header("authorization").is_none());
        } else {
            assert!(request.target == "/v1/api/torrents/mylist?id=7123&bypass_cache=true");
            assert!(
                request.header("authorization")
                    == Some(format!("Bearer {}", fixtures::TOKEN).as_str())
            );
            if index >= 2 {
                let (started, finished) = open_windows[index - 2];
                assert!((started..=finished).contains(&request.received_at));
            }
        }
    }
}
