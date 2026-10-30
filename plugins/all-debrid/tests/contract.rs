#[path = "fixtures/wave_audio.rs"]
mod audio;
#[path = "fixtures/bound_wav_workflow.rs"]
mod bound_wav_workflow;
#[path = "fixtures/detail_budget.rs"]
mod detail_budget;
#[path = "fixtures/http.rs"]
mod http;
#[path = "fixtures/logging.rs"]
mod logging;
use hito_debrid_all_debrid::test_support::media;
#[path = "fixtures/preparation_guards.rs"]
mod preparation_guards;
#[path = "fixtures/recovery.rs"]
mod recovery;
#[path = "fixtures/retry.rs"]
mod retry;
mod support;

use hito_debrid_all_debrid::MagnetFileBinding;
use hito_plugin_sdk::{ProviderRecording, SourcePage};
use reqwest::{Method, StatusCode};
use serde_json::json;

const PROVIDER_TOKEN: &str = "owned-private-ad-service-provider-token";
const SERVICE_TOKEN: &str = "owned-private-ad-service-bearer-token";
const WRONG_TOKEN: &str = "owned-private-ad-service-wrong-token";
const COOKIE: &str = "owned-private-ad-caller-cookie";
const QUERY: &str = "owned-private-ad";
const TITLE: &str = "owned-private-ad-title";
const ARTIST: &str = "owned-private-ad-artist";
const RECORDING: &str = "curated-recording-1";
const SOURCE: &str = "curated-wav-source-1";
const MEDIA_HOST: &str = "media.hito-fixture.invalid";
const MEDIA_TARGET: &str = "/owned-private-service.wav?signature=owned-private-service-signature";
const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
const DIRECTORY: &str = "owned-private-directory";
const FILE: &str = "owned-private-service.wav";
const SELECTED_LINK: &str = "https://alldebrid.com/f/owned-private-service-selected";
const HANDLE_KEY: [u8; 32] = [0x73; 32];

async fn sources(
    client: &reqwest::Client,
    plugin: &http::Server,
    statuses: &mut Vec<u16>,
) -> SourcePage {
    let (code, body) = http::json(
        client,
        plugin,
        Method::POST,
        "v1/sources",
        Some(SERVICE_TOKEN),
        Some(json!({"recording_id":RECORDING})),
    )
    .await;
    statuses.push(code.as_u16());
    assert_eq!(code, StatusCode::OK);
    private_json(&body);
    let page: SourcePage = serde_json::from_slice(&body).unwrap();
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, SOURCE);
    assert_eq!(page.items[0].label, "Curated canonical PCM16 WAV");
    page
}

fn private_json(bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).unwrap();
    for private in [
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        WRONG_TOKEN,
        COOKIE,
        HASH,
        DIRECTORY,
        FILE,
        SELECTED_LINK,
        MEDIA_TARGET,
        "http://",
        "https://",
    ] {
        assert!(!text.contains(private));
        let escaped = serde_json::to_string(private).unwrap();
        assert!(!text.contains(&escaped[1..escaped.len() - 1]));
    }
}

fn recording() -> ProviderRecording {
    ProviderRecording {
        isrc: None,
        id: RECORDING.into(),
        title: TITLE.into(),
        artist: ARTIST.into(),
        duration_ms: Some(2000),
        release: None,
        disc_number: None,
        track_number: None,
        revision: Some("curated-wav-v1".into()),
        attribution: None,
    }
}

fn prepared_replies(binding: &MagnetFileBinding, delayed: u64, link: &str) -> Vec<Vec<u8>> {
    [
        json!({"status":"success","data":{"magnets":[{
            "id":binding.magnet_id,"size":binding.magnet_bytes,"statusCode":4
        }]}}),
        json!({"status":"success","data":{"magnets":[{
            "id":binding.magnet_id.to_string(),"files":[
                {"n":&binding.file_path[0],"e":[{"n":&binding.file_path[1],"s":binding.file_bytes,"l":link}]},
                {"n":"other","e":[{"n":"decoy.wav","s":binding.file_bytes,
                    "l":"https://alldebrid.com/f/owned-private-retarget-decoy"}]},
                {"n":"readme.txt","s":400,"l":"https://alldebrid.com/f/owned-private-retarget-note"}
            ]
        }]}}),
        json!({"status":"success","data":{"delayed":delayed,"filesize":binding.file_bytes}}),
    ].into_iter().map(|value| serde_json::to_vec(&value).unwrap()).collect()
}
