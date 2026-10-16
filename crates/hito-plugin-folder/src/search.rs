use crate::catalog::Catalog;
use axum::http::StatusCode;
use hito_plugin_sdk::{SearchPage, SearchRequest};
use sha2::{Digest, Sha256};

pub(crate) fn page(catalog: &Catalog, request: SearchRequest) -> Result<SearchPage, StatusCode> {
    request.validate().map_err(|_| StatusCode::BAD_REQUEST)?;
    let query = request.query.to_lowercase();
    let signature = format!(
        "{:x}",
        Sha256::digest(format!("{}\0{query}", catalog.fingerprint))
    );
    let prefix = format!("c-{signature}-");
    let offset = match request.cursor {
        None => 0,
        Some(cursor) => {
            let number = cursor
                .strip_prefix(&prefix)
                .ok_or(StatusCode::BAD_REQUEST)?;
            if number.len() != 8 || !number.bytes().all(|value| value.is_ascii_hexdigit()) {
                return Err(StatusCode::BAD_REQUEST);
            }
            usize::from_str_radix(number, 16).map_err(|_| StatusCode::BAD_REQUEST)?
        }
    };
    let matches: Vec<_> = catalog
        .tracks
        .values()
        .filter(|track| track.searchable.contains(&query))
        .collect();
    if offset > matches.len() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut items = Vec::new();
    let mut bytes = 512;
    for track in matches.iter().skip(offset).take(usize::from(request.limit)) {
        let recording = track.recording();
        let length = serde_json::to_vec(&recording)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .len()
            + 1;
        if bytes + length > 262144 {
            break;
        }
        bytes += length;
        items.push(recording);
    }
    let next = offset + items.len();
    let next_cursor = (next < matches.len()).then(|| format!("{prefix}{next:08x}"));
    Ok(SearchPage { items, next_cursor })
}
