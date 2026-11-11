use axum::http::HeaderMap;
use std::time::{Duration, SystemTime};

pub(crate) fn retry_delay(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let mut values = headers.get_all("retry-after").iter();
    let Some(value) = values.next() else {
        return Some(Duration::from_secs(60));
    };
    if values.next().is_some() {
        return None;
    }
    let value = value.to_str().ok()?;
    let delay = if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        Duration::from_secs(value.parse::<u64>().ok()?)
    } else {
        httpdate::parse_http_date(value)
            .ok()?
            .duration_since(now)
            .unwrap_or(Duration::ZERO)
    };
    Some(delay.max(Duration::from_secs(1)))
}

#[cfg(test)]
mod tests {
    use super::retry_delay;
    use axum::http::{HeaderMap, HeaderValue};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn retry_headers_use_the_supplied_clock_and_reject_ambiguous_values() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut headers = HeaderMap::new();
        assert_eq!(retry_delay(&headers, now), Some(Duration::from_secs(60)));
        for (value, expected) in [
            ("3".to_owned(), Some(Duration::from_secs(3))),
            ("0".to_owned(), Some(Duration::from_secs(1))),
            (
                httpdate::fmt_http_date(now + Duration::from_secs(600)),
                Some(Duration::from_secs(600)),
            ),
            (
                httpdate::fmt_http_date(now - Duration::from_secs(60)),
                Some(Duration::from_secs(1)),
            ),
            ("invalid private value".to_owned(), None),
            ("18446744073709551616".to_owned(), None),
        ] {
            headers.insert("retry-after", HeaderValue::from_str(&value).unwrap());
            assert_eq!(retry_delay(&headers, now), expected);
        }
        headers.insert("retry-after", HeaderValue::from_static("60"));
        headers.append("retry-after", HeaderValue::from_static("120"));
        assert_eq!(retry_delay(&headers, now), None);
    }
}
