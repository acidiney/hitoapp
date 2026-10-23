use super::{
    CHANNELS, FILE_NAME, FOLDER_ID, FRAMES, ITEM_CREATED, ITEM_ID, MEDIA_ORIGIN, MEDIA_TARGET,
    SAMPLE_RATE, TOKEN, TRANSFER_CREATED, TRANSFER_ID,
    fixture::{
        Fixture, Request, Response,
        logs::{Capture, Expected},
    },
    item_details, owned_wave, transfer_list,
};
use hito_debrid_premiumize::{
    Error, MediaAllowlist, OpenedBoundAudio, PremiumizeClient, TransferBinding, TransferFileBinding,
};
use serde_json::Value;
use tracing::instrument::WithSubscriber;

mod binding;
mod lifecycle;
mod media;
mod metadata;

fn binding() -> TransferFileBinding {
    TransferFileBinding {
        transfer: TransferBinding {
            transfer_id: TRANSFER_ID.into(),
            created_at_unix_seconds: TRANSFER_CREATED,
        },
        item_id: ITEM_ID.into(),
        item_created_at_unix_seconds: ITEM_CREATED,
        file_name: FILE_NAME.into(),
        file_bytes: owned_wave().len() as u64,
        folder_id: FOLDER_ID.into(),
    }
}

fn transfers() -> Value {
    serde_json::from_slice(&transfer_list()).unwrap()
}
fn item() -> Value {
    serde_json::from_slice(&item_details(binding().file_bytes)).unwrap()
}
fn json(value: &Value) -> Response {
    Response::json(serde_json::to_vec(value).unwrap())
}

struct Journey {
    api: Fixture,
    media: Fixture,
    client: PremiumizeClient,
    allowlist: MediaAllowlist,
}

impl Journey {
    async fn start(api: Vec<Response>, media: Vec<Response>) -> Self {
        let api = Fixture::responses("www.premiumize.me", api).await;
        let media = Fixture::responses("media.hito-fixture.invalid", media).await;
        let client = PremiumizeClient::with_test_tls_media(
            TOKEN.into(),
            api.ca_der(),
            api.address(),
            media.ca_der(),
            media.address(),
        )
        .unwrap();
        let allowlist = MediaAllowlist::new(vec![MEDIA_ORIGIN.into()]).unwrap();
        Self {
            api,
            media,
            client,
            allowlist,
        }
    }

    async fn close(self, api_count: usize, media_count: usize) -> Vec<Request> {
        let api = self.api.close().await;
        let media = self.media.close().await;
        assert_eq!(api.len(), api_count);
        assert_eq!(media.len(), media_count);
        for request in &api {
            assert!(request.server_name == "www.premiumize.me");
            assert!(request.method == "GET");
            assert!(
                request.target == "/api/transfer/list"
                    || request.target == format!("/api/item/details?id={ITEM_ID}")
            );
            assert!(request.header("host") == Some("www.premiumize.me"));
            assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
            assert!(request.header("accept") == Some("application/json"));
            assert!(request.header("accept-encoding") == Some("identity"));
            for header in [
                "cookie",
                "referer",
                "apikey",
                "pin",
                "proxy-authorization",
                "content-type",
            ] {
                assert!(request.header(header).is_none());
            }
        }
        for request in media {
            assert!(request.server_name == "media.hito-fixture.invalid");
            assert!(request.method == "GET" && request.target == MEDIA_TARGET);
            assert!(request.header("host") == Some("media.hito-fixture.invalid"));
            assert!(request.header("accept") == Some("audio/wav"));
            assert!(request.header("accept-encoding") == Some("identity"));
            for header in [
                "authorization",
                "cookie",
                "referer",
                "range",
                "proxy-authorization",
                "apikey",
                "pin",
                "x-hito-plugin-protocol",
            ] {
                assert!(request.header(header).is_none());
            }
        }
        api
    }
}

fn rejected(result: Result<OpenedBoundAudio, Error>, expected: Error) {
    match result {
        Err(error) => assert_eq!(error, expected),
        Ok(_) => panic!("the unsupported bound file unexpectedly opened audio"),
    }
}

async fn collect(mut body: OpenedBoundAudio, expected: &TransferFileBinding) -> Vec<u8> {
    assert!(body.binding == *expected);
    assert_eq!(body.length, expected.file_bytes);
    let hito_remote_audio::AudioFormat::Pcm16Wav(wav_format) = body.format else {
        panic!("the unchanged WAV policy must return a WAV descriptor");
    };
    assert_eq!(wav_format.sample_rate, SAMPLE_RATE);
    assert_eq!(wav_format.channels, CHANNELS);
    assert_eq!(wav_format.frames, u64::from(FRAMES));
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next_chunk().await.unwrap() {
        assert!(!chunk.is_empty() && chunk.len() <= 65_536);
        assert!(bytes.len() + chunk.len() <= expected.file_bytes as usize);
        bytes.extend_from_slice(&chunk);
    }
    assert!(body.next_chunk().await.unwrap().is_none());
    assert!(bytes == owned_wave());
    bytes
}

fn logs(capture: &Capture, expected: &[Expected]) {
    capture.assert_events(
        expected,
        &["owned", "www.premiumize.me", "media.hito-fixture.invalid"],
    );
}

async fn reject_metadata(
    response: Response,
    at_item: bool,
    expected: Error,
    outcome: &'static str,
) {
    let mut responses = if at_item {
        vec![json(&transfers())]
    } else {
        Vec::new()
    };
    responses.push(response);
    let journey = Journey::start(responses, vec![Response::wave(owned_wave())]).await;
    let capture = Capture::default();
    rejected(
        journey
            .client
            .open_bound_audio(&binding(), &journey.allowlist)
            .with_subscriber(capture.dispatch())
            .await,
        expected,
    );
    journey.close(if at_item { 2 } else { 1 }, 0).await;
    logs(&capture, &[Expected::Opening(outcome)]);
}
