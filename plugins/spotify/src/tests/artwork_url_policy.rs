use super::Transport;
use crate::{Config, error::Error};

#[test]
fn image_network_endpoints_require_the_exact_cdn_path_and_canonical_https_authority() {
    let transport = Transport::new(&Config::default()).unwrap();
    let image = "0123456789abcdef0123456789abcdef01234567";
    let mosaic = image.repeat(4);
    for (label, location, allowed) in [
        (
            "AK recording cover",
            format!("https://image-cdn-ak.spotifycdn.com/image/{image}"),
            true,
        ),
        (
            "FA recording cover",
            format!("https://image-cdn-fa.spotifycdn.com/image/{image}"),
            true,
        ),
        (
            "small mosaic",
            format!("https://mosaic.scdn.co/60/{mosaic}"),
            true,
        ),
        (
            "medium mosaic",
            format!("https://mosaic.scdn.co/300/{mosaic}"),
            true,
        ),
        (
            "large mosaic",
            format!("https://mosaic.scdn.co/640/{mosaic}"),
            true,
        ),
        (
            "maximum mosaic",
            format!("https://mosaic.scdn.co/1024/{mosaic}"),
            true,
        ),
        (
            "mosaic path on AK",
            format!("https://image-cdn-ak.spotifycdn.com/640/{mosaic}"),
            false,
        ),
        (
            "mosaic path on FA",
            format!("https://image-cdn-fa.spotifycdn.com/640/{mosaic}"),
            false,
        ),
        (
            "recording path on mosaic",
            format!("https://mosaic.scdn.co/image/{image}"),
            false,
        ),
        (
            "host suffix",
            format!("https://mosaic.scdn.co.owned.invalid/640/{mosaic}"),
            false,
        ),
        (
            "subdomain",
            format!("https://cdn.mosaic.scdn.co/640/{mosaic}"),
            false,
        ),
        (
            "trailing host dot",
            format!("https://mosaic.scdn.co./640/{mosaic}"),
            false,
        ),
        (
            "host case alias",
            format!("https://MOSAIC.SCDN.CO/640/{mosaic}"),
            false,
        ),
        (
            "default port alias",
            format!("https://mosaic.scdn.co:443/640/{mosaic}"),
            false,
        ),
        (
            "alternate port",
            format!("https://mosaic.scdn.co:444/640/{mosaic}"),
            false,
        ),
        (
            "plain HTTP",
            format!("http://mosaic.scdn.co/640/{mosaic}"),
            false,
        ),
        (
            "userinfo",
            format!("https://owned:private@mosaic.scdn.co/640/{mosaic}"),
            false,
        ),
        (
            "empty userinfo",
            format!("https://@mosaic.scdn.co/640/{mosaic}"),
            false,
        ),
        (
            "empty query",
            format!("https://mosaic.scdn.co/640/{mosaic}?"),
            false,
        ),
        (
            "empty fragment",
            format!("https://mosaic.scdn.co/640/{mosaic}#"),
            false,
        ),
        (
            "normalized dot segment",
            format!("https://mosaic.scdn.co/640/../640/{mosaic}"),
            false,
        ),
        (
            "percent encoded size",
            format!("https://mosaic.scdn.co/%36%34%30/{mosaic}"),
            false,
        ),
    ] {
        let result = transport.image_endpoint(&location);
        if allowed {
            assert_eq!(result.unwrap().as_str(), location, "case: {label}");
        } else {
            assert!(
                matches!(result, Err(Error::InvalidResponse)),
                "case: {label}"
            );
        }
    }
}
