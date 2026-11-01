pub use super::observed::ObservedRequest;
use super::{covers::COVER_A, observed::observed};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use hito_plugin_sdk::{
    ARTWORK_PATH, Capability, FEATURED_PATH, FeaturedItem, FeaturedKind, FeaturedPage,
    FeaturedRequest, Manifest, PROTOCOL_VERSION,
};
use hito_testkit::fake_plugin::{self as fake, FakePlugin, Fault};
use serde_json::Value;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

const REQUEST_LIMIT: usize = 64;

#[derive(Clone)]
pub enum Reply {
    Items(Vec<FeaturedItem>),
    Raw(Value),
    Status(StatusCode),
}

#[derive(Clone)]
pub enum Cover {
    Png,
    Bytes(Vec<u8>, &'static str),
    Missing,
}

struct Behavior {
    reply: Reply,
    cover: Cover,
}

pub struct FeaturedServer {
    pub url: String,
    plugin: FakePlugin,
    behavior: Arc<Mutex<Behavior>>,
}

pub fn item(id: &str) -> FeaturedItem {
    FeaturedItem {
        id: id.into(),
        title: format!("Featured {id}"),
        subtitle: Some(format!("By {id}")),
        kind: FeaturedKind::Playlist,
        reference: format!("https://provider.example/playlist/{id}"),
        artwork_id: Some(format!("art-{id}")),
        item_count: Some(12),
    }
}

impl FeaturedServer {
    pub async fn start() -> Self {
        Self::with_capabilities(
            "org.hito.featured-fixture",
            "Owned featured service",
            [
                Capability::Playlists,
                Capability::Featured,
                Capability::Artwork,
            ],
        )
        .await
    }

    pub async fn without_featured() -> Self {
        Self::with_capabilities(
            "org.hito.no-featured-fixture",
            "Owned playlist service",
            [Capability::Playlists],
        )
        .await
    }

    pub async fn with_capabilities(
        id: &str,
        name: &str,
        capabilities: impl IntoIterator<Item = Capability>,
    ) -> Self {
        let behavior = Arc::new(Mutex::new(Behavior {
            reply: Reply::Items(Vec::new()),
            cover: Cover::Png,
        }));
        let plugin = FakePlugin::builder(Manifest {
            id: id.into(),
            name: name.into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: capabilities.into_iter().collect(),
            authorization: None,
            content_policy: None,
        })
        .post(FEATURED_PATH, {
            let behavior = behavior.clone();
            move |request| {
                let Some(parsed) = serde_json::from_value::<FeaturedRequest>(request.json())
                    .ok()
                    .filter(|parsed| parsed.validate().is_ok())
                else {
                    return fake::Reply::Refuse;
                };
                match &behavior.lock().unwrap().reply {
                    Reply::Items(items) => fake::Reply::Json(
                        serde_json::to_value(FeaturedPage {
                            items: items
                                .iter()
                                .take(usize::from(parsed.limit))
                                .cloned()
                                .collect(),
                        })
                        .unwrap(),
                    ),
                    Reply::Raw(value) => fake::Reply::Json(value.clone()),
                    Reply::Status(status) => fake::Reply::Status(*status),
                }
            }
        })
        .post(ARTWORK_PATH, {
            let behavior = behavior.clone();
            move |_| match behavior.lock().unwrap().cover.clone() {
                Cover::Png => image(COVER_A.to_vec(), "image/png"),
                Cover::Bytes(bytes, content_type) => image(bytes, content_type),
                Cover::Missing => fake::Reply::Status(StatusCode::NOT_FOUND),
            }
        })
        .strict(REQUEST_LIMIT)
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            plugin,
            behavior,
        }
    }

    pub fn reply(&self, reply: Reply) {
        self.behavior.lock().unwrap().reply = reply;
    }

    pub fn cover(&self, cover: Cover) {
        self.behavior.lock().unwrap().cover = cover;
    }

    pub fn delay_featured(&self, delay: Duration) {
        self.plugin.clear_fault(FEATURED_PATH);
        if !delay.is_zero() {
            self.plugin.fault(FEATURED_PATH, Fault::Delay(delay));
        }
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        observed(&self.plugin, REQUEST_LIMIT)
    }

    pub fn featured_requests(&self) -> Vec<ObservedRequest> {
        self.requests_for(FEATURED_PATH)
    }

    pub fn artwork_requests(&self) -> Vec<ObservedRequest> {
        self.requests_for(ARTWORK_PATH)
    }

    fn requests_for(&self, path: &str) -> Vec<ObservedRequest> {
        self.requests()
            .into_iter()
            .filter(|request| request.path == format!("/prefix/{path}"))
            .collect()
    }

    pub fn unexpected(&self) -> usize {
        self.plugin.unexpected()
    }

    pub async fn finish(&mut self) -> bool {
        self.plugin.finish().await
    }
}

fn image(body: Vec<u8>, content_type: &'static str) -> fake::Reply {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static(content_type));
    fake::Reply::Raw {
        status: StatusCode::OK,
        headers,
        body,
        chunked: false,
    }
}
