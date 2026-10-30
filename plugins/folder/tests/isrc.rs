#[allow(dead_code)]
mod support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_plugin_folder::Folder;
use hito_plugin_sdk::SearchPage;
use http_body_util::BodyExt;
use lofty::{
    config::WriteOptions,
    tag::{Accessor, ItemKey, Tag, TagExt, TagType},
};
use tower::ServiceExt;

fn tag_isrc(path: &std::path::Path, title: &str, isrc: &str) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    assert!(tag.insert_text(ItemKey::Isrc, isrc.into()));
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

async fn search(app: axum::Router) -> SearchPage {
    let response = app
        .oneshot(
            Request::post("/v1/search")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"isrc","limit":20}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page: SearchPage =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    page.validate(20).unwrap();
    page
}

#[tokio::test]
async fn a_tagged_isrc_is_published_in_canonical_form_and_malformed_codes_are_omitted() {
    let directory = tempfile::tempdir().unwrap();
    for (name, title, isrc, frequency) in [
        ("a.wav", "isrc canonical", Some("us-rc1-76-07839"), 440),
        ("b.wav", "isrc malformed", Some("not an isrc"), 554),
        ("c.wav", "isrc untagged", None, 659),
    ] {
        let path = directory.path().join(name);
        support::write_wav(&path, Some(title), frequency);
        if let Some(isrc) = isrc {
            tag_isrc(&path, title, isrc);
        }
    }
    let page = search(Folder::scan(directory.path()).unwrap().router()).await;
    let isrc = |title: &str| {
        page.items
            .iter()
            .find(|item| item.title == title)
            .unwrap_or_else(|| panic!("{title} is listed"))
            .isrc
            .clone()
    };
    assert_eq!(isrc("isrc canonical").as_deref(), Some("USRC17607839"));
    assert_eq!(isrc("isrc malformed"), None);
    assert_eq!(isrc("isrc untagged"), None);
}
