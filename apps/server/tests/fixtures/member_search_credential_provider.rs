use crate::{catalog_album_header_fixture as catalog, member_search_order_library::MEMBER};
use axum::http::StatusCode;
use hito_plugin_sdk::{
    ArtistProfileRequest, ArtistSearchPage, ArtistSearchRequest, Capability, Manifest,
    PROTOCOL_VERSION,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::json;

pub(crate) const TOKEN: &str = "owned-member-search-order-token";
pub(crate) const DISCOVERY: &str = "v1/catalog/discovery/capabilities";
pub(crate) const SEARCH: &str = "v1/catalog/artists/search";

#[derive(Clone, Copy)]
pub(crate) enum SearchOutcome {
    Available,
    Rejected,
}

pub(crate) fn request() -> ArtistSearchRequest {
    ArtistSearchRequest {
        query: MEMBER.into(),
        cursor: None,
        snapshot: None,
        limit: 2,
    }
}

pub(crate) fn page() -> ArtistSearchPage {
    ArtistSearchPage {
        query: MEMBER.into(),
        snapshot: None,
        items: Vec::new(),
        total_count: Some(0),
        next_cursor: None,
    }
}

pub(crate) async fn start(outcome: SearchOutcome) -> FakePlugin {
    catalog::group().validate().unwrap();
    page().validate_for(&request()).unwrap();
    FakePlugin::builder(Manifest {
        id: "org.hito.member-search-dispatch-order".into(),
        name: "Owned member search ordering catalog".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Catalog].into(),
        authorization: None,
        content_policy: None,
    })
    .get("v1/catalog/capabilities", |_| {
        Reply::Json(json!({"catalog_version": 1, "operations": ["artist_profile"]}))
    })
    .post("v1/catalog/artists/profile", |request| {
        let parsed: ArtistProfileRequest = serde_json::from_value(request.json()).unwrap();
        assert!(parsed.validate().is_ok());
        assert_eq!(parsed.artist_id, catalog::ARTIST);
        Reply::Json(json!({
            "artist": {"id": catalog::ARTIST, "name": "Owned header performer"},
            "sort_name": null, "kind": null, "disambiguation": null, "revision": null,
        }))
    })
    .get(DISCOVERY, |_| {
        Reply::Json(json!({"discovery_version": 1, "operations": ["artist_search"]}))
    })
    .post(SEARCH, move |recorded| {
        let parsed: ArtistSearchRequest = serde_json::from_value(recorded.json()).unwrap();
        parsed.validate().unwrap();
        assert_eq!(json!(parsed), json!(request()));
        match outcome {
            SearchOutcome::Available => Reply::Json(json!(page())),
            SearchOutcome::Rejected => Reply::Status(StatusCode::UNAUTHORIZED),
        }
    })
    .get(catalog::GROUP_CAPABILITIES, |_| {
        Reply::Json(json!({"release_group_version": 1, "operations": ["release_group"]}))
    })
    .post(catalog::GROUP_DETAIL, |request| {
        assert_eq!(request.json(), json!({"release_group_id": catalog::GROUP}));
        Reply::Json(json!(catalog::group()))
    })
    .require_protocol()
    .strict(24)
    .start()
    .await
}
