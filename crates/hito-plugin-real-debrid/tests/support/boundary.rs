use crate::support::{
    ARTIST, COOKIE, FILE, HASH, HOSTER_LINK, MEDIA_HOST, MEDIA_TARGET, PROVIDER_TOKEN, QUERY,
    SERVICE_TOKEN, TITLE, TORRENT, WRONG_TOKEN, audio, http,
    logging::Capture,
    provider::{Exchange, Provider},
};
use hito_debrid_real_debrid::{MediaAllowlist, test_support::owned_protocol_client};
use hito_plugin_real_debrid::{Config, CuratedWavRecording, router};
use reqwest::{Method, RequestBuilder, StatusCode};
use serde_json::Value;
use std::time::Duration;

pub const CHANGED_FILE: &str = "/private-changed-directory/private-changed-file-canary.wav";

pub struct Fixture {
    pub provider: Provider,
    pub plugin: http::Server,
    pub client: reqwest::Client,
    capture: Capture,
}

impl Fixture {
    pub async fn start(recordings: Vec<CuratedWavRecording>) -> Self {
        let provider = Provider::start(audio::wav()).await;
        let capture = Capture::default();
        let client = owned_protocol_client(
            PROVIDER_TOKEN.into(),
            provider.ca_der(),
            provider.api_address,
            provider.media_address,
        )
        .expect("owned protocol client must initialize");
        let config = Config::new(
            SERVICE_TOKEN.into(),
            client,
            MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).expect("owned media origin"),
            recordings,
        )
        .expect("curated fixture catalog must validate");
        let plugin = http::Server::start(
            router(config).expect("plugin router must initialize"),
            capture.dispatch(),
        )
        .await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(4))
            .build()
            .expect("owned HTTP client");
        Self {
            provider,
            plugin,
            client,
            capture,
        }
    }

    pub fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.client
            .request(
                method,
                self.plugin.base.join(path).expect("owned plugin path"),
            )
            .bearer_auth(SERVICE_TOKEN)
            .header("cookie", COOKIE)
    }

    pub async fn close(self) -> (Capture, Vec<Exchange>) {
        let (plugin, provider) =
            futures_util::future::join(self.plugin.close(), self.provider.close()).await;
        plugin.expect("owned plugin must join shutdown");
        let exchanges = provider.expect("all owned TLS peers must join shutdown");
        (self.capture, exchanges)
    }
}

pub async fn response(request: RequestBuilder, expected: StatusCode) -> Vec<u8> {
    let mut response = request
        .send()
        .await
        .expect("owned HTTP request must finish");
    assert_eq!(response.status(), expected);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    for header in ["location", "set-cookie", "content-disposition", "etag"] {
        assert!(
            response.headers().get(header).is_none(),
            "private headers must remain absent"
        );
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .expect("owned response must reach EOF")
    {
        assert!(
            chunk.len() <= 65_536_usize.saturating_sub(bytes.len()),
            "response exceeded its bound"
        );
        bytes.extend_from_slice(&chunk);
    }
    assert_private(&bytes, false);
    bytes
}

pub async fn rejected(request: RequestBuilder, expected: StatusCode, reason: &str) {
    let bytes = response(request, expected).await;
    assert_private(&bytes, true);
    let error: Value = serde_json::from_slice(&bytes).expect("error must be structured JSON");
    assert_eq!(error["reason"], reason);
    assert!(
        error["error"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
    );
}

pub fn assert_private(bytes: &[u8], exclude_metadata: bool) {
    let text = std::str::from_utf8(bytes).expect("control response must be UTF-8");
    for forbidden in [
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        WRONG_TOKEN,
        COOKIE,
        TORRENT,
        HASH,
        FILE,
        HOSTER_LINK,
        MEDIA_TARGET,
        "http://",
        "https://",
        "private-malformed-body-canary",
        "private-unknown-recording-canary",
        "private-unknown-source-canary",
        CHANGED_FILE,
    ] {
        assert!(
            !text.contains(forbidden),
            "response exposed a private canary"
        );
    }
    if exclude_metadata {
        for forbidden in [TITLE, ARTIST, QUERY] {
            assert!(
                !text.contains(forbidden),
                "error echoed private musical metadata"
            );
        }
    }
}

pub fn private_log_canaries() -> [&'static str; 18] {
    [
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        WRONG_TOKEN,
        COOKIE,
        TORRENT,
        HASH,
        FILE,
        HOSTER_LINK,
        MEDIA_TARGET,
        TITLE,
        ARTIST,
        QUERY,
        "http://",
        "https://",
        "private-malformed-body-canary",
        "private-unknown-recording-canary",
        "private-unknown-source-canary",
        CHANGED_FILE,
    ]
}
