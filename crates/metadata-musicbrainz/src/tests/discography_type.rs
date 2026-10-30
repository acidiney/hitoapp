use super::{
    ARTIST_ID,
    http::{Fixture, Reply},
};
use crate::{Error, MusicBrainzClient};
use axum::http::StatusCode;
use hito_plugin_sdk::{ArtistDiscographyRequest, CatalogReleaseTypeFilter};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const FIRST: &str = "0f740000-0000-4000-8000-000000000001";
const SECOND: &str = "0f740000-0000-4000-8000-000000000002";
const THIRD: &str = "0f740000-0000-4000-8000-000000000003";

fn group(id: &str, kind: &str) -> Value {
    json!({"id": id, "title": "Owned typed title", "primary-type": kind, "secondary-types": [],
        "first-release-date": "2020"})
}

fn reply(offset: u32, count: u32, groups: Vec<Value>) -> Reply {
    Reply::json(
        serde_json::to_vec(&json!({
            "release-group-count": count,
            "release-group-offset": offset,
            "release-groups": groups
        }))
        .expect("the owned typed browse reply must serialize"),
    )
}

fn request(
    release_type: Option<CatalogReleaseTypeFilter>,
    cursor: Option<String>,
) -> ArtistDiscographyRequest {
    ArtistDiscographyRequest {
        artist_id: ARTIST_ID.into(),
        cursor,
        snapshot: None,
        limit: 2,
        release_type,
    }
}

fn query(endpoint: &reqwest::Url, uri: &axum::http::Uri) -> BTreeMap<String, String> {
    endpoint
        .join(&uri.to_string())
        .expect("the observed request must be a URL")
        .query_pairs()
        .into_owned()
        .collect()
}

#[tokio::test]
async fn a_typed_read_pages_only_that_type_with_a_cursor_that_cannot_change_type() {
    let fixture = Fixture::start(vec![
        reply(0, 3, vec![group(FIRST, "Album"), group(SECOND, "Album")]),
        reply(2, 3, vec![group(THIRD, "Album")]),
        reply(0, 1, vec![group(FIRST, "Other")]),
    ])
    .await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned typed client must initialize");

    let first = client
        .artist_discography(request(Some(CatalogReleaseTypeFilter::Album), None))
        .await
        .expect("the first typed page must load");
    let cursor = first
        .next_cursor
        .clone()
        .expect("a second album page exists");
    let second = client
        .artist_discography(request(
            Some(CatalogReleaseTypeFilter::Album),
            Some(cursor.clone()),
        ))
        .await
        .expect("the typed continuation must load");
    assert!(second.next_cursor.is_none());
    assert_eq!(first.items.len() + second.items.len(), 3);

    let before = fixture.request_count();
    for (filter, token) in [
        (Some(CatalogReleaseTypeFilter::Ep), cursor.clone()),
        (None, cursor.clone()),
    ] {
        assert_eq!(
            client
                .artist_discography(request(filter, Some(token)))
                .await
                .err(),
            Some(Error::InvalidDiscographyRequest),
            "a typed cursor must not continue another type or the whole catalog"
        );
    }
    assert_eq!(
        fixture.request_count(),
        before,
        "a refused continuation must make no request"
    );

    let other = client
        .artist_discography(request(Some(CatalogReleaseTypeFilter::Other), None))
        .await
        .expect("the other type must load");
    assert_eq!(other.items.len(), 1);

    let requests = fixture.requests();
    let endpoint = fixture.endpoint.clone();
    drop(client);
    fixture.close().await;
    assert_eq!(
        requests.len(),
        3,
        "each read is exactly one bounded request"
    );
    for (uri, (kind, offset)) in
        requests
            .iter()
            .zip([("album", "0"), ("album", "2"), ("other", "0")])
    {
        assert_eq!(uri.path(), "/ws/2/release-group");
        assert_eq!(
            query(&endpoint, uri),
            BTreeMap::from([
                ("artist".into(), ARTIST_ID.into()),
                ("fmt".into(), "json".into()),
                ("inc".into(), "artist-credits".into()),
                ("limit".into(), "2".into()),
                ("offset".into(), offset.into()),
                ("type".into(), kind.into()),
            ])
        );
    }
}

#[tokio::test]
async fn a_whole_catalog_cursor_cannot_continue_a_typed_read() {
    let fixture = Fixture::start(vec![reply(
        0,
        3,
        vec![group(FIRST, "Album"), group(SECOND, "EP")],
    )])
    .await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned typed client must initialize");
    let cursor = client
        .artist_discography(request(None, None))
        .await
        .expect("the whole catalog page must load")
        .next_cursor
        .expect("more groups remain");
    assert_eq!(
        client
            .artist_discography(request(Some(CatalogReleaseTypeFilter::Album), Some(cursor)))
            .await
            .err(),
        Some(Error::InvalidDiscographyRequest)
    );
    assert_eq!(fixture.request_count(), 1);
    drop(client);
    fixture.close().await;
}

#[tokio::test]
async fn a_typed_read_rejects_other_types_and_reports_a_provider_outage() {
    let mut outage = reply(0, 0, Vec::new());
    outage.status = StatusCode::SERVICE_UNAVAILABLE;
    let fixture = Fixture::start(vec![
        reply(0, 2, vec![group(FIRST, "Album"), group(SECOND, "EP")]),
        reply(0, 1, vec![group(THIRD, "Album")]),
        outage,
    ])
    .await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned typed client must initialize");
    assert_eq!(
        client
            .artist_discography(request(Some(CatalogReleaseTypeFilter::Album), None))
            .await
            .err(),
        Some(Error::InvalidDiscography),
        "a group of another type must invalidate a typed page"
    );
    assert_eq!(
        client
            .artist_discography(request(Some(CatalogReleaseTypeFilter::Single), None))
            .await
            .err(),
        Some(Error::InvalidDiscography)
    );
    assert_eq!(
        client
            .artist_discography(request(Some(CatalogReleaseTypeFilter::Ep), None))
            .await
            .err(),
        Some(Error::Status(503)),
        "an outage stays a provider failure"
    );
    drop(client);
    fixture.close().await;
}
