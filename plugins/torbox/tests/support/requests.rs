use super::{fixtures, media_fixture::MediaRequest, provider::Request};
use std::time::{Duration, Instant};

pub fn exact_authorities(
    requests: &[Request],
    media: &[MediaRequest],
    prepared: Instant,
    opened: Instant,
) {
    assert_eq!(requests.len(), 7);
    assert_eq!(media.len(), 1);
    assert!(requests[0].received_at.into_std() >= prepared);
    assert!(
        requests[1]
            .received_at
            .duration_since(requests[0].received_at)
            >= Duration::from_secs(5)
    );
    for (index, request) in requests.iter().enumerate() {
        assert!(request.method == "GET" && request.server_name == "api.torbox.app");
        assert!(request.header("host") == Some("api.torbox.app"));
        assert!(request.header("accept") == Some("application/json"));
        assert!(request.header("accept-encoding") == Some("identity"));
        assert!(request.body.is_empty());
        for name in [
            "cookie",
            "referer",
            "range",
            "proxy-authorization",
            "content-type",
            "x-hito-plugin-protocol",
        ] {
            assert!(request.header(name).is_none());
        }
        if index == 2 {
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
        }
    }
    let request = &media[0];
    assert!(
        request.method == "GET"
            && request.server_name == fixtures::MEDIA_HOST
            && request.target == fixtures::MEDIA_TARGET
    );
    assert!(request.header("host") == Some(fixtures::MEDIA_HOST));
    assert!(request.header("accept") == Some("audio/wav"));
    assert!(request.header("accept-encoding") == Some("identity"));
    assert!(
        request.received_at.into_std() >= opened && request.received_at >= requests[5].received_at
    );
    for name in [
        "authorization",
        "cookie",
        "referer",
        "range",
        "proxy-authorization",
        "x-hito-plugin-protocol",
    ] {
        assert!(request.header(name).is_none());
    }
}
