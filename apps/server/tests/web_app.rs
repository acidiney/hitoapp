use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const ICONS: &[(&str, &str)] = &[
    ("favicon.ico", "image/x-icon"),
    ("favicon.svg", "image/svg+xml"),
    ("favicon-16.png", "image/png"),
    ("favicon-32.png", "image/png"),
    ("apple-touch-icon.png", "image/png"),
    ("icon-192.png", "image/png"),
    ("icon-512.png", "image/png"),
    ("icon-maskable-512.png", "image/png"),
    ("icon.svg", "image/svg+xml"),
];

async fn app(directory: &std::path::Path) -> Router {
    let services = hito_core::open(hito_core::CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        hito_core::HostDeviceId::new("web-app-test-host").unwrap(),
    ))
    .await
    .unwrap();
    hito_server::router_with_config(
        services,
        hito_server::Config {
            web_dir: directory.join("web"),
            ..Default::default()
        },
    )
}

async fn request(app: &Router, method: &str, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn manifest_is_available_before_setup_for_get_and_head() {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir(&web).unwrap();
    let manifest = br#"{"name":"Hito","start_url":"/"}"#;
    std::fs::write(web.join("site.webmanifest"), manifest).unwrap();
    let app = app(directory.path()).await;

    for method in ["GET", "HEAD"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/site.webmanifest")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{method}");
        assert_eq!(
            response.headers()["content-type"],
            "application/manifest+json"
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert!(response.headers().contains_key("content-security-policy"));
        assert!(response.headers().contains_key("x-request-id"));
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let expected: &[u8] = if method == "HEAD" { b"" } else { manifest };
        assert_eq!(body.as_ref(), expected);
    }
}

#[tokio::test]
async fn root_service_worker_revalidates_without_changing_private_cache_policy() {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir(&web).unwrap();
    let worker = b"self.addEventListener('install', () => self.skipWaiting());";
    std::fs::write(web.join("sw.js"), worker).unwrap();
    let app = app(directory.path()).await;
    for method in ["GET", "HEAD"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/sw.js")
                    .header("x-inertia", "true")
                    .header("x-inertia-version", "stale")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{method}");
        assert_eq!(response.headers()["content-type"], "text/javascript");
        assert_eq!(response.headers()["cache-control"], "no-cache");
        assert_eq!(response.headers()["service-worker-allowed"], "/");
        assert_eq!(
            response.headers()["content-length"],
            worker.len().to_string()
        );
        assert!(response.headers().get("x-inertia").is_none());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let expected: &[u8] = if method == "HEAD" { b"" } else { worker };
        assert_eq!(body.as_ref(), expected);
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
}

#[tokio::test]
async fn only_the_shipped_icons_are_served_from_the_web_root() {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir(&web).unwrap();
    for (filename, _) in ICONS {
        std::fs::write(web.join(filename), format!("owned fixture:{filename}")).unwrap();
    }
    std::fs::write(web.join("private.txt"), "private web file").unwrap();
    let app = app(directory.path()).await;
    for (filename, content_type) in ICONS {
        for method in ["GET", "HEAD"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(format!("/{filename}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{method} {filename}");
            assert_eq!(response.headers()["content-type"], *content_type);
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            assert_eq!(response.headers()["x-content-type-options"], "nosniff");
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let expected = if method == "HEAD" {
                String::new()
            } else {
                format!("owned fixture:{filename}")
            };
            assert_eq!(body.as_ref(), expected.as_bytes());
        }
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/private.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(!String::from_utf8_lossy(&body).contains("private web file"));
}

#[tokio::test]
async fn missing_web_roots_files_and_directories_remain_plain_not_found_responses() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path()).await;
    let web = directory.path().join("web");
    for stage in ["missing_root", "missing_files", "directories"] {
        if stage == "missing_files" {
            std::fs::create_dir(&web).unwrap();
        }
        for filename in ICONS
            .iter()
            .map(|(name, _)| *name)
            .chain(["site.webmanifest", "sw.js"])
        {
            if stage == "directories" {
                std::fs::create_dir(web.join(filename)).unwrap();
                std::fs::write(web.join(filename).join("private.txt"), "directory secret").unwrap();
            }
            for method in ["GET", "HEAD"] {
                let response = request(&app, method, &format!("/{filename}")).await;
                assert_eq!(
                    response.status(),
                    StatusCode::NOT_FOUND,
                    "{stage} {filename}"
                );
                assert_eq!(response.headers()["cache-control"], "private, no-store");
                assert!(response.headers().get("x-inertia").is_none());
                let body = response.into_body().collect().await.unwrap().to_bytes();
                let expected: &[u8] = if method == "HEAD" { b"" } else { b"Not found" };
                assert_eq!(body.as_ref(), expected, "{stage} {filename}");
            }
        }
    }
    let response = request(&app, "GET", "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("LoginPage"));
}

#[cfg(unix)]
#[tokio::test]
async fn root_assets_cannot_follow_symlinks_or_traverse_to_private_files() {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir(&web).unwrap();
    let secret = "private outside build root";
    let private = directory.path().join("private.txt");
    std::fs::write(&private, secret).unwrap();
    for (index, filename) in ICONS
        .iter()
        .map(|(name, _)| *name)
        .chain(["site.webmanifest", "sw.js"])
        .enumerate()
    {
        let target = if index % 2 == 0 {
            private.clone()
        } else {
            std::path::PathBuf::from("../private.txt")
        };
        std::os::unix::fs::symlink(target, web.join(filename)).unwrap();
    }
    let app = app(directory.path()).await;
    let paths = ICONS.iter().map(|(name, _)| format!("/{name}")).chain(
        [
            "/site.webmanifest",
            "/sw.js",
            "/../private.txt",
            "/%2e%2e/private.txt",
            "/favicon.svg/../../private.txt",
            "/favicon.svg%2f..%2fprivate.txt",
            "/private.txt",
        ]
        .map(str::to_owned),
    );
    for path in paths {
        for method in ["GET", "HEAD"] {
            let response = request(&app, method, &path).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let body = String::from_utf8_lossy(&body);
            assert!(!body.contains(secret), "{method} {path}");
            assert!(!body.contains("data-page"), "{method} {path}");
            assert!(!body.contains(directory.path().to_str().unwrap()));
        }
    }
}

#[tokio::test]
async fn root_assets_keep_the_existing_origin_and_method_policy() {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir(&web).unwrap();
    std::fs::write(web.join("sw.js"), "owned worker").unwrap();
    let app = app(directory.path()).await;
    for (origin, expected) in [
        ("http://localhost:8080", StatusCode::METHOD_NOT_ALLOWED),
        ("https://untrusted.invalid", StatusCode::FORBIDDEN),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sw.js")
                    .header("origin", origin)
                    .header("x-hito-request", "1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_fifo_at_an_allowlisted_path_is_rejected_without_a_blocked_reader() {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir(&web).unwrap();
    let fifo = web.join("sw.js");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let app = app(directory.path()).await;
    let mut response = tokio::spawn(async move { request(&app, "GET", "/sw.js").await });
    let (response, blocked) =
        match tokio::time::timeout(std::time::Duration::from_secs(1), &mut response).await {
            Ok(response) => (response.unwrap(), false),
            Err(_) => {
                let release = tokio::task::spawn_blocking(move || {
                    std::fs::OpenOptions::new().write(true).open(fifo)
                });
                let response = response.await.unwrap();
                drop(release.await.unwrap().unwrap());
                (response, true)
            }
        };
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(
        !blocked,
        "FIFO blocked the reader, then was positively released and joined"
    );
}

#[tokio::test]
async fn page_head_supports_installation_without_weakening_csp_or_initial_state_escaping() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path()).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/setup")
                .header("x-hito-request", "1")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"username":"admin","password":"web app test password"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let setup: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let token = setup["token"].as_str().unwrap();
    let display_name = "</script><script>escape()</script>&";
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/account/display-name")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"display_name":display_name}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/library")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let csp = response.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(csp.contains("script-src 'self';"));
    assert!(csp.contains("default-src 'self';"));
    assert!(csp.contains("frame-ancestors 'none';"));
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = std::str::from_utf8(&body).unwrap();
    let required_head_tags = [
        r#"<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content">"#,
        r#"<link rel="manifest" href="/site.webmanifest">"#,
        r#"<link rel="icon" href="/favicon.ico" sizes="48x48">"#,
        r#"<link rel="icon" href="/favicon.svg" type="image/svg+xml" sizes="any">"#,
        r#"<link rel="icon" href="/favicon-16.png" type="image/png" sizes="16x16">"#,
        r#"<link rel="icon" href="/favicon-32.png" type="image/png" sizes="32x32">"#,
        r#"<link rel="apple-touch-icon" href="/apple-touch-icon.png" sizes="180x180">"#,
        r##"<meta name="theme-color" media="(prefers-color-scheme: light)" content="#fbfbfa">"##,
        r##"<meta name="theme-color" media="(prefers-color-scheme: dark)" content="#111110">"##,
        r#"<meta name="mobile-web-app-capable" content="yes">"#,
        r#"<meta name="apple-mobile-web-app-capable" content="yes">"#,
        r#"<meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">"#,
        r#"<meta name="apple-mobile-web-app-title" content="Hito">"#,
    ];
    let missing_head_tags: Vec<_> = required_head_tags
        .into_iter()
        .filter(|tag| !html.contains(tag))
        .collect();
    assert!(
        missing_head_tags.is_empty(),
        "Missing head tags: {missing_head_tags:?}"
    );
    assert!(!html.contains(display_name));
    let encoded = html
        .split_once(r#"<script data-page="app" type="application/json">"#)
        .unwrap()
        .1
        .split_once("</script>")
        .unwrap()
        .0;
    let page: Value = serde_json::from_str(encoded).unwrap();
    assert_eq!(page["props"]["user"]["display_name"], display_name);
    assert_eq!(html.matches("<script").count(), 2);
}
