use axum::{Router, extract::State, http::header, routing::get};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone)]
struct Feed {
    xml: Arc<tokio::sync::RwLock<String>>,
    feed_requests: Arc<AtomicUsize>,
    enclosure_requests: Arc<AtomicUsize>,
}

#[derive(Clone, Copy, Debug)]
pub enum NumberOverflow {
    EnclosureLength,
    Duration,
}

pub struct PodcastServer {
    pub origin: String,
    pub feed_url: String,
    xml: Arc<tokio::sync::RwLock<String>>,
    feed_requests: Arc<AtomicUsize>,
    enclosure_requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl PodcastServer {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let feed_url = format!("{origin}/feed.xml");
        let feed_requests = Arc::new(AtomicUsize::new(0));
        let enclosure_requests = Arc::new(AtomicUsize::new(0));
        let xml = Arc::new(tokio::sync::RwLock::new(snapshot(
            &origin,
            "First owned episode",
            "first.wav",
        )));
        let feed = Feed {
            xml: xml.clone(),
            feed_requests: feed_requests.clone(),
            enclosure_requests: enclosure_requests.clone(),
        };
        let app = Router::new()
            .route("/feed.xml", get(read_feed))
            .route("/first.wav", get(read_enclosure))
            .route("/second.wav", get(read_enclosure))
            .with_state(feed);
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            origin,
            feed_url,
            xml,
            feed_requests,
            enclosure_requests,
            task,
        }
    }

    pub fn feed_requests(&self) -> usize {
        self.feed_requests.load(Ordering::SeqCst)
    }

    pub fn enclosure_requests(&self) -> usize {
        self.enclosure_requests.load(Ordering::SeqCst)
    }

    pub async fn revise_episode(&self) {
        *self.xml.write().await = snapshot(&self.origin, "Revised owned episode", "second.wav");
    }

    pub async fn publish_numeric_overflow(&self, overflow: NumberOverflow) {
        let xml = snapshot(&self.origin, "Rejected revised episode", "second.wav");
        let xml = match overflow {
            NumberOverflow::EnclosureLength => xml.replace("length=\"16044\"", "length=\"9223372036854775808\""),
            NumberOverflow::Duration => xml
                .replace(
                    "<rss version=\"2.0\">",
                    "<rss version=\"2.0\" xmlns:itunes=\"http://www.itunes.com/dtds/podcast-1.0.dtd\">",
                )
                .replace("</item>", "<itunes:duration>9223372036854776</itunes:duration></item>"),
        };
        *self.xml.write().await = xml;
    }

    pub async fn publish_two_episodes(&self, author: &str) {
        let xml = snapshot(&self.origin, "First owned episode", "first.wav")
            .replace(
                "<rss version=\"2.0\">",
                "<rss version=\"2.0\" xmlns:itunes=\"http://www.itunes.com/dtds/podcast-1.0.dtd\">",
            )
            .replace(
                "<description>An owned RSS fixture.</description>",
                &format!("<description>An owned RSS fixture.</description><itunes:author>{author}</itunes:author>"),
            )
            .replace(
                "</channel>",
                &format!(
                    r#"<item><guid isPermaLink="false">episode-opaque-two</guid>
<title>Second owned episode</title>
<enclosure url="{}/second.wav" length="16044" type="audio/wav" />
</item></channel>"#,
                    self.origin
                ),
            );
        *self.xml.write().await = xml;
    }
}

impl Drop for PodcastServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_feed(State(feed): State<Feed>) -> ([(header::HeaderName, &'static str); 1], String) {
    feed.feed_requests.fetch_add(1, Ordering::SeqCst);
    let xml = feed.xml.read().await.clone();
    ([(header::CONTENT_TYPE, "application/rss+xml")], xml)
}

fn snapshot(origin: &str, title: &str, enclosure: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel>
<title>Owned podcast</title>
<link>{origin}/</link>
<description>An owned RSS fixture.</description>
<item><guid isPermaLink="false">episode-opaque-one</guid>
<title>{title}</title>
<description>Audio created for this test.</description>
<enclosure url="{origin}/{enclosure}" length="16044" type="audio/wav" />
</item></channel></rss>"#
    )
}

async fn read_enclosure(State(feed): State<Feed>) -> axum::http::StatusCode {
    feed.enclosure_requests.fetch_add(1, Ordering::SeqCst);
    axum::http::StatusCode::SERVICE_UNAVAILABLE
}
