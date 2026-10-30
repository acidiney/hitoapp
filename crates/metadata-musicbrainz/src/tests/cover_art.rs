use crate::{CoverArtClient, CoverKind, Error};
use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    response::Response,
};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

const MBID: &str = "0f700000-0000-4000-8000-0000000000aa";

#[derive(Clone)]
enum Reply {
    Image(&'static str, Vec<u8>),
    Missing,
    Redirect(String),
}

type Seen = Arc<Mutex<Vec<String>>>;

async fn serve(route: Reply) -> (reqwest::Url, Seen) {
    let seen: Seen = Default::default();
    let state = (route, seen.clone());
    let router = Router::new()
        .fallback(
            |State((route, seen)): State<(Reply, Seen)>, request: Request| async move {
                seen.lock().unwrap().push(request.uri().to_string());
                if request.uri().path().starts_with("/final") {
                    return image("image/jpeg", vec![0xff, 0xd8, 1, 2, 3]);
                }
                match route {
                    Reply::Image(kind, bytes) => image(kind, bytes),
                    Reply::Missing => Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body("".into())
                        .unwrap(),
                    Reply::Redirect(to) => Response::builder()
                        .status(StatusCode::TEMPORARY_REDIRECT)
                        .header(header::LOCATION, to)
                        .body("".into())
                        .unwrap(),
                }
            },
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (endpoint, seen)
}

fn image(kind: &str, bytes: Vec<u8>) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, kind)
        .body(bytes.into())
        .unwrap()
}

#[tokio::test]
async fn a_front_cover_is_read_by_identifier_only() {
    let (endpoint, seen) = serve(Reply::Image("image/jpeg", vec![0xff, 0xd8, 9])).await;
    let client = CoverArtClient::with_loopback_endpoint(endpoint).unwrap();
    let cover = client
        .front(CoverKind::ReleaseGroup, MBID)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cover.content_type, "image/jpeg");
    assert_eq!(
        seen.lock().unwrap().clone(),
        vec![format!("/release-group/{MBID}/front-500")]
    );
}

#[tokio::test]
async fn an_absent_cover_is_not_an_error() {
    let (endpoint, _) = serve(Reply::Missing).await;
    let client = CoverArtClient::with_loopback_endpoint(endpoint).unwrap();
    assert!(
        client
            .front(CoverKind::Release, MBID)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_redirect_inside_the_service_is_followed_and_one_elsewhere_is_refused() {
    let (endpoint, seen) = serve(Reply::Redirect("/final/image".into())).await;
    let client = CoverArtClient::with_loopback_endpoint(endpoint).unwrap();
    assert!(
        client
            .front(CoverKind::Release, MBID)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(seen.lock().unwrap().len(), 2);

    let (endpoint, seen) = serve(Reply::Redirect("http://127.0.0.2:9/x".into())).await;
    let client = CoverArtClient::with_loopback_endpoint(endpoint).unwrap();
    assert_eq!(
        client.front(CoverKind::Release, MBID).await.unwrap_err(),
        Error::InvalidResponse
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "The foreign address is never contacted"
    );
}

#[tokio::test]
async fn other_content_oversize_bodies_and_bad_identifiers_are_refused() {
    let (endpoint, _) = serve(Reply::Image("text/html", b"<html>".to_vec())).await;
    let client = CoverArtClient::with_loopback_endpoint(endpoint).unwrap();
    assert_eq!(
        client.front(CoverKind::Release, MBID).await.unwrap_err(),
        Error::InvalidResponse
    );
    let (endpoint, _) = serve(Reply::Image("image/png", vec![0; 3 * 1024 * 1024])).await;
    let client = CoverArtClient::with_loopback_endpoint(endpoint).unwrap();
    assert_eq!(
        client.front(CoverKind::Release, MBID).await.unwrap_err(),
        Error::ResponseTooLarge
    );
    let (endpoint, seen) = serve(Reply::Missing).await;
    let client = CoverArtClient::with_loopback_endpoint(endpoint).unwrap();
    for bad in [
        "",
        "../x",
        "0F700000-0000-4000-8000-0000000000AA",
        "not-an-id",
    ] {
        assert_eq!(
            client.front(CoverKind::Release, bad).await.unwrap_err(),
            Error::InvalidLookupRequest
        );
    }
    assert!(seen.lock().unwrap().is_empty());
    assert!(
        CoverArtClient::with_loopback_endpoint("https://example.com/".parse().unwrap()).is_err()
    );
}
