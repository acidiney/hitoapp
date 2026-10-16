use hito_desktop::{LocalHost, Paths};
use reqwest::{Client, StatusCode};
use serde_json::json;

#[tokio::test]
async fn only_the_bootstrapped_window_can_access_the_local_application() {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    let host = LocalHost::start(Paths {
        data: directory.path().join("data"),
        music: directory.path().join("music"),
        web,
    })
    .await
    .unwrap();
    let url = host.entry_url();
    let root = url.join("/").unwrap();
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    assert_eq!(
        client
            .get(root.join("health").unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .post(root.join("api/v1/setup").unwrap())
            .header("x-hito-request", "1")
            .json(&json!({"username":"unowned","password":"unowned startup test password"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let bootstrap = client.get(url.clone()).send().await.unwrap();
    assert_eq!(bootstrap.status(), StatusCode::SEE_OTHER);
    assert_eq!(bootstrap.headers()["location"], "/library");
    let cookie = bootstrap.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    let cookie = cookie.split(';').next().unwrap();
    assert_eq!(
        client.get(url).send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let page = client
        .get(root.join("library").unwrap())
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert!(page.text().await.unwrap().contains("LoginPage"));
    let setup = client
        .post(root.join("api/v1/setup").unwrap())
        .header("cookie", cookie)
        .header("x-hito-request", "1")
        .header("origin", root.origin().ascii_serialization())
        .json(&json!({"username":"owner","password":"local startup test password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(setup.status(), StatusCode::CREATED);
    assert_eq!(
        client
            .get(root.join("health").unwrap())
            .header("cookie", cookie)
            .header("host", "rebound.invalid")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .get(root.join("health").unwrap())
            .header("cookie", cookie)
            .header("origin", "https://unrelated.invalid")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    host.shutdown().await.unwrap();
    assert!(
        client
            .get(root.join("health").unwrap())
            .send()
            .await
            .is_err()
    );
}
