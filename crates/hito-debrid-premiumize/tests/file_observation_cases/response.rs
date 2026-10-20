use super::*;
use hito_debrid_premiumize::ProviderRejection;
use serde_json::json as value;

#[tokio::test]
async fn both_api_stages_keep_http_and_business_errors_private_without_retries() {
    for at_item in [false, true] {
        for status in [401, 403, 429, 500] {
            let mut response =
                json(&value!({"status":"error","message":"owned private provider failure"}));
            response.status = status;
            reject_response(response, at_item, Error::Status(status), "http_rejected").await;
        }
        for (code, reason) in [
            (
                "authentication_failed",
                ProviderRejection::AuthenticationFailed,
            ),
            ("permission_denied", ProviderRejection::PermissionDenied),
            ("rate_limit_reached", ProviderRejection::RateLimited),
            ("not_found", ProviderRejection::NotFound),
            ("owned-unknown-private-code", ProviderRejection::Rejected),
        ] {
            reject_response(json(&value!({"status":"error","code":code,"message":"owned private provider failure"})), at_item,
                Error::ProviderRejected(reason), "provider_rejected").await;
        }
    }
}

#[tokio::test]
async fn both_api_stages_bound_json_headers_and_bodies_without_media() {
    for at_item in [false, true] {
        reject_response(
            Response::json(vec![b'x'; 65_537]),
            at_item,
            Error::ResponseTooLarge,
            "response_too_large",
        )
        .await;
        reject_response(
            Response::json(b"{owned-private-malformed-body".to_vec()),
            at_item,
            Error::InvalidResponse,
            "invalid_response",
        )
        .await;
        let good = if at_item { item() } else { transfers() };
        let mut response = json(&good);
        response.headers[0].1 = "text/html".into();
        reject_response(
            response,
            at_item,
            Error::InvalidResponse,
            "invalid_response",
        )
        .await;
        let mut response = json(&good);
        response
            .headers
            .push(("Content-Type".into(), "application/json".into()));
        reject_response(
            response,
            at_item,
            Error::InvalidResponse,
            "invalid_response",
        )
        .await;
        let mut response = json(&good);
        response
            .headers
            .push(("Content-Encoding".into(), "gzip".into()));
        reject_response(
            response,
            at_item,
            Error::InvalidResponse,
            "invalid_response",
        )
        .await;
        let mut response = json(&good);
        for index in 0..29 {
            response
                .headers
                .push((format!("X-Owned-{index}"), "owned-private-header".into()));
        }
        reject_response(
            response,
            at_item,
            Error::InvalidResponse,
            "invalid_response",
        )
        .await;
        let mut response = json(&good);
        response
            .headers
            .push(("X-Owned-Large".into(), "x".repeat(9000)));
        reject_response(
            response,
            at_item,
            Error::InvalidResponse,
            "invalid_response",
        )
        .await;
        let mut response = json(&good);
        response.status = 302;
        response.headers.push((
            "Location".into(),
            "https://unapproved.example.invalid/owned-private-redirect".into(),
        ));
        reject_response(response, at_item, Error::Status(302), "http_rejected").await;
        let mut value = good;
        value["code"] = "owned-unexpected-private-code".into();
        reject_response(
            json(&value),
            at_item,
            Error::InvalidResponse,
            "invalid_response",
        )
        .await;
    }
}
