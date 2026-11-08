use crate::Error;

const VIDEO_UNAVAILABLE: &[&str] = &[
    "private video",
    "video unavailable",
    "this video is not available",
    "this video has been removed",
    "this video is no longer available",
    "not made this video available in your country",
    "blocked it in your country",
    "who has blocked it",
    "members-only",
    "account associated with this video has been terminated",
];

const FORMAT_MISSING: &[&str] = &["requested format is not available"];

const REFUSED: &[&str] = &[
    "sign in to confirm",
    "not a bot",
    "http error 403",
    "http error 429",
    "too many requests",
];

const NETWORK: &[&str] = &[
    "name resolution",
    "name or service not known",
    "nodename nor servname",
    "failed to resolve",
    "getaddrinfo",
    "network is unreachable",
    "no route to host",
    "connection refused",
    "connection reset",
    "connection timed out",
    "timed out",
    "transporterror",
    "unable to connect",
];

pub(crate) fn failure(stderr: &[u8]) -> Error {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    let contains = |markers: &[&str]| markers.iter().any(|marker| text.contains(marker));
    if contains(VIDEO_UNAVAILABLE) {
        Error::VideoUnavailable
    } else if contains(FORMAT_MISSING) {
        Error::FormatMissing
    } else if contains(REFUSED) {
        Error::Refused
    } else if contains(NETWORK) {
        Error::Network
    } else {
        Error::Unavailable
    }
}
