use sha2::{Digest, Sha256};

pub fn assert_private_logs(
    capture: &crate::logs::Logs,
    library: &crate::fixture::Library,
    provider: &crate::provider::Provider,
    outcomes: &[&str],
    extra: &[&str],
) {
    let cookie = library.account.headers()["cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    let token = cookie.strip_prefix("hito_session=").unwrap();
    let session_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let mut canaries = vec![
        crate::provider::PROVIDER_ID,
        crate::provider::TOKEN,
        crate::provider::TITLE,
        crate::provider::LINE,
        crate::provider::RECORDING_REVISION,
        crate::provider::DOCUMENT_REVISION,
        "owned-music-private-path-canary",
        "owned-original.wav",
        &provider.url,
        &library.recording,
        &library.observation,
        &cookie,
        token,
        &session_hash,
    ];
    canaries.extend_from_slice(extra);
    capture.assert_private(&canaries, outcomes);
}
