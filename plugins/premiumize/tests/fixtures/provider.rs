use super::{
    wave_fixture,
    wire_fixture::{Fixture, Request, Response},
};
use hito_debrid_premiumize::{
    MediaAllowlist, PremiumizeClient, TransferBinding, TransferFileBinding,
};
use hito_plugin_premiumize::{ConnectionConfig, CuratedWavRecording};
use hito_plugin_sdk::ProviderRecording;
use serde_json::json;

pub const SERVICE: &str = "owned-private-premiumize-service-bearer-a";
pub const PEER_SERVICE: &str = "owned-private-premiumize-service-bearer-b";
pub const WRONG_SERVICE: &str = "owned-private-premiumize-service-bearer-wrong";
pub const PROVIDER: &str = "owned-private-premiumize-provider-bearer-a";
pub const PEER_PROVIDER: &str = "owned-private-premiumize-provider-bearer-b";
pub const COOKIE: &str = "owned-private-browser-session-cookie";
pub const QUERY: &str = "owned-private-visible";
pub const TITLE: &str = "owned-private-visible exact curated song";
pub const PEER_TITLE: &str = "owned-private-peer curated song";
pub const ARTIST: &str = "owned-private-curated performer";
pub const RECORDING: &str = "curated-recording-a";
pub const SOURCE: &str = "curated-source-a";
pub const PEER_RECORDING: &str = "curated-recording-b";
pub const PEER_SOURCE: &str = "curated-source-b";
pub const LABEL: &str = "Curated canonical PCM16 WAV";
pub const TRANSFER: &str = "owned-private-premiumize-transfer";
pub const ITEM: &str = "owned-private-premiumize-item";
pub const FOLDER: &str = "owned-private-premiumize-folder";
pub const FILE: &str = "owned-private-exact-cloud-file.wav";
pub const TRANSFER_CREATED: u64 = 1_700_000_000;
pub const ITEM_CREATED: u64 = TRANSFER_CREATED + 120;
pub const MEDIA_HOST: &str = "media.hito-fixture.invalid";
pub const LINK_A: &str =
    "https://media.hito-fixture.invalid/owned-private-readiness-link?token=owned-private-link-a";
pub const LINK_B: &str =
    "https://media.hito-fixture.invalid/owned-private-fresh-audio-link?token=owned-private-link-b";

pub struct Provider {
    pub api: Fixture,
    pub media: Fixture,
    pub peer_api: Fixture,
    pub peer_media: Fixture,
}

impl Provider {
    pub async fn start() -> Self {
        let api = Fixture::responses(
            "www.premiumize.me",
            vec![
                transfer("queued", 0.0),
                transfer("queued", 0.0),
                transfer("finished", 1.0),
                transfer("finished", 1.0),
                item(LINK_A),
                transfer("finished", 1.0),
                item(LINK_B),
            ],
        )
        .await;
        let media = Fixture::responses(MEDIA_HOST, vec![Response::wave(wave_fixture::wav())]).await;
        let peer_api =
            Fixture::responses("www.premiumize.me", vec![transfer("finished", 1.0)]).await;
        let peer_media =
            Fixture::responses(MEDIA_HOST, vec![Response::wave(wave_fixture::wav())]).await;
        Self {
            api,
            media,
            peer_api,
            peer_media,
        }
    }

    pub fn connections(&self) -> Vec<ConnectionConfig> {
        [false, true]
            .into_iter()
            .map(|peer| {
                let (api, media, provider, service) = if peer {
                    (
                        &self.peer_api,
                        &self.peer_media,
                        PEER_PROVIDER,
                        PEER_SERVICE,
                    )
                } else {
                    (&self.api, &self.media, PROVIDER, SERVICE)
                };
                ConnectionConfig {
                    service_bearer: service.into(),
                    receipt_key: [if peer { 0x72 } else { 0x71 }; 32],
                    client: PremiumizeClient::with_test_tls_media(
                        provider.into(),
                        api.ca_der(),
                        api.address(),
                        media.ca_der(),
                        media.address(),
                    )
                    .unwrap(),
                    media_origins: MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")])
                        .unwrap(),
                    recordings: vec![CuratedWavRecording {
                        recording: recording(peer),
                        source_id: if peer { PEER_SOURCE } else { SOURCE }.into(),
                        label: LABEL.into(),
                        binding: binding(),
                    }],
                }
            })
            .collect()
    }

    pub fn counts(&self) -> [usize; 4] {
        [
            self.api.request_count(),
            self.media.request_count(),
            self.peer_api.request_count(),
            self.peer_media.request_count(),
        ]
    }

    pub async fn close(self) -> (Vec<Request>, Vec<Request>, Vec<Request>, Vec<Request>) {
        let api = self.api.close().await;
        let media = self.media.close().await;
        let peer_api = self.peer_api.close().await;
        let peer_media = self.peer_media.close().await;
        (api, media, peer_api, peer_media)
    }
}

pub fn recording(peer: bool) -> ProviderRecording {
    ProviderRecording {
        isrc: None,
        artist_urls: Vec::new(),
        id: if peer { PEER_RECORDING } else { RECORDING }.into(),
        title: if peer { PEER_TITLE } else { TITLE }.into(),
        artist: ARTIST.into(),
        duration_ms: Some(2000),
        release: None,
        disc_number: None,
        track_number: None,
        revision: Some("curated-file-v1".into()),
        attribution: None,
    }
}

pub fn binding() -> TransferFileBinding {
    TransferFileBinding {
        transfer: TransferBinding {
            transfer_id: TRANSFER.into(),
            created_at_unix_seconds: TRANSFER_CREATED,
        },
        item_id: ITEM.into(),
        item_created_at_unix_seconds: ITEM_CREATED,
        file_name: FILE.into(),
        file_bytes: wave_fixture::WAV_BYTES as u64,
        folder_id: FOLDER.into(),
    }
}

fn transfer(status: &str, progress: f64) -> Response {
    let available = matches!(status, "finished" | "seeding");
    Response::json(serde_json::to_vec(&json!({"status":"success", "transfers":[
        {"id":"owned-private-same-name-decoy", "name":FILE, "created_at":TRANSFER_CREATED,
         "status":"finished", "progress":1.0, "file_id":"owned-private-decoy-item", "folder_id":FOLDER},
        {"id":TRANSFER, "name":FILE, "created_at":TRANSFER_CREATED, "status":status, "progress":progress,
         "file_id":if available { Some(ITEM) } else { None }, "folder_id":if available { Some(FOLDER) } else { None },
         "message":"owned-private-provider-transfer-message"}
    ]})).unwrap())
}

fn item(link: &str) -> Response {
    Response::json(
        serde_json::to_vec(&json!({"status":"success", "id":ITEM, "name":FILE,
        "size":wave_fixture::WAV_BYTES, "created_at":ITEM_CREATED, "folder_id":FOLDER, "link":link,
        "future_field":"owned-private-provider-item-message"}))
        .unwrap(),
    )
}

pub fn assert_protocol(exchanges: (Vec<Request>, Vec<Request>, Vec<Request>, Vec<Request>)) {
    let (api, media, peer_api, peer_media) = exchanges;
    assert_eq!(api.len(), 7);
    for (request, target) in api.iter().zip([
        "/api/transfer/list".into(),
        "/api/transfer/list".into(),
        "/api/transfer/list".into(),
        "/api/transfer/list".into(),
        format!("/api/item/details?id={ITEM}"),
        "/api/transfer/list".into(),
        format!("/api/item/details?id={ITEM}"),
    ]) {
        assert!(request.server_name == "www.premiumize.me");
        assert!(request.method == "GET" && request.target == target);
        assert!(request.header("host") == Some("www.premiumize.me"));
        assert!(request.header("authorization") == Some(format!("Bearer {PROVIDER}").as_str()));
        assert!(request.header("accept") == Some("application/json"));
        assert!(request.header("accept-encoding") == Some("identity"));
        for name in [
            "cookie",
            "referer",
            "content-type",
            "apikey",
            "pin",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ] {
            assert!(request.header(name).is_none());
        }
    }
    assert_eq!(media.len(), 1);
    let request = &media[0];
    assert!(request.server_name == MEDIA_HOST);
    assert!(
        request.method == "GET"
            && request.target
                == LINK_B
                    .strip_prefix("https://media.hito-fixture.invalid")
                    .unwrap()
    );
    assert!(request.header("host") == Some(MEDIA_HOST));
    assert!(request.header("accept") == Some("audio/wav"));
    assert!(request.header("accept-encoding") == Some("identity"));
    for name in [
        "authorization",
        "cookie",
        "referer",
        "range",
        "proxy-authorization",
        "apikey",
        "pin",
        "x-hito-plugin-protocol",
    ] {
        assert!(request.header(name).is_none());
    }
    assert!(peer_api.is_empty() && peer_media.is_empty());
}

pub fn assert_private_wire(bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).unwrap();
    for private in [
        SERVICE,
        PEER_SERVICE,
        WRONG_SERVICE,
        PROVIDER,
        PEER_PROVIDER,
        COOKIE,
        TRANSFER,
        ITEM,
        FOLDER,
        FILE,
        LINK_A,
        LINK_B,
        "owned-private-link-a",
        "owned-private-link-b",
        "http://",
        "https://",
        "owned-private-provider-transfer-message",
        "owned-private-provider-item-message",
    ] {
        let escaped = serde_json::to_string(private).unwrap();
        assert!(!text.contains(private) && !text.contains(&escaped[1..escaped.len() - 1]));
    }
}
