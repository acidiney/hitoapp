use super::{
    BYTES, FOLDER, ITEM, ITEM_CREATED, LINK, NAME, TOKEN, TRANSFER, TRANSFER_CREATED, item_details,
    logs::Capture,
    transfer_list,
    wire_fixture::{Fixture, Request, Response},
};
use hito_debrid_premiumize::{
    BoundFileObservation, BoundFilePhase, Error, PremiumizeClient, TransferBinding,
    TransferFileBinding,
};
use serde_json::Value;
use tracing::instrument::WithSubscriber;

mod authority;
mod lifecycle;
mod media_slots;
mod response;
mod wave;

fn binding() -> TransferFileBinding {
    TransferFileBinding {
        transfer: TransferBinding {
            transfer_id: TRANSFER.into(),
            created_at_unix_seconds: TRANSFER_CREATED,
        },
        item_id: ITEM.into(),
        item_created_at_unix_seconds: ITEM_CREATED,
        file_name: NAME.into(),
        file_bytes: BYTES,
        folder_id: FOLDER.into(),
    }
}

fn transfers() -> Value {
    serde_json::from_slice(&transfer_list("finished", 1.0)).unwrap()
}
fn item() -> Value {
    serde_json::from_slice(&item_details()).unwrap()
}
fn json(value: &Value) -> Response {
    Response::json(serde_json::to_vec(value).unwrap())
}

#[derive(Clone, Copy)]
enum ApiCall {
    Transfers,
    Item,
}

struct Journey {
    api: Fixture,
    media: Fixture,
    client: PremiumizeClient,
}

impl Journey {
    async fn start(api: Vec<Response>) -> Self {
        Self::with_media(api, vec![Response::wave(vec![0; 44])]).await
    }

    async fn with_media(api: Vec<Response>, media: Vec<Response>) -> Self {
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
        Self { api, media, client }
    }

    async fn close(self, expected_api: &[ApiCall], media_count: usize) {
        let requests = self.api.close().await;
        let media = self.media.close().await;
        assert_eq!(requests.len(), expected_api.len());
        assert_eq!(media.len(), media_count);
        for (request, expected) in requests.iter().zip(expected_api) {
            assert_api(request, *expected);
        }
        for request in media {
            assert!(request.server_name == "media.hito-fixture.invalid");
            assert!(
                request.method == "GET"
                    && request.target
                        == LINK
                            .strip_prefix("https://media.hito-fixture.invalid")
                            .unwrap()
            );
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
    }
}

fn assert_api(request: &Request, expected: ApiCall) {
    let target = match expected {
        ApiCall::Transfers => "/api/transfer/list".into(),
        ApiCall::Item => format!("/api/item/details?id={ITEM}"),
    };
    assert!(request.server_name == "www.premiumize.me");
    assert!(request.method == "GET" && request.target == target);
    assert!(request.header("host") == Some("www.premiumize.me"));
    assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
    assert!(request.header("accept") == Some("application/json"));
    assert!(request.header("accept-encoding") == Some("identity"));
    for header in [
        "cookie",
        "referer",
        "content-type",
        "apikey",
        "pin",
        "proxy-authorization",
    ] {
        assert!(request.header(header).is_none());
    }
}

fn available(observed: BoundFileObservation) {
    let expected = binding();
    assert!(observed.binding == expected);
    assert_eq!(
        serde_json::to_vec(&observed.binding).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    assert_eq!(observed.phase, BoundFilePhase::ProviderCompleted);
    assert_eq!(observed.progress.fraction(), 1.0);
}

fn rejected(result: Result<BoundFileObservation, Error>, expected: Error) {
    match result {
        Err(error) => assert_eq!(error, expected),
        Ok(_) => panic!("an unavailable or unverified file produced an available observation"),
    }
}

fn logs(capture: &Capture, outcomes: &[&str]) {
    capture.assert_outcomes(
        outcomes,
        &["owned", "www.premiumize.me", "media.hito-fixture.invalid"],
    );
}

async fn reject_response(
    response: Response,
    at_item: bool,
    expected: Error,
    outcome: &'static str,
) {
    let mut api = if at_item {
        vec![json(&transfers())]
    } else {
        Vec::new()
    };
    api.push(response);
    let journey = Journey::start(api).await;
    let capture = Capture::default();
    rejected(
        journey
            .client
            .observe_bound_file(&binding())
            .with_subscriber(capture.dispatch())
            .await,
        expected,
    );
    let calls = if at_item {
        vec![ApiCall::Transfers, ApiCall::Item]
    } else {
        vec![ApiCall::Transfers]
    };
    journey.close(&calls, 0).await;
    logs(&capture, &[outcome]);
}
