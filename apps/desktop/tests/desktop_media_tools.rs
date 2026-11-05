use hito_desktop::{LocalHost, Paths};
use reqwest::Client;
use serde_json::{Value, json};
use std::time::Duration;

#[tokio::test]
async fn a_scan_without_ffmpeg_tools_tells_the_user_to_install_them() {
    let directory = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HITO_FFPROBE", directory.path().join("missing-ffprobe"));
        std::env::set_var("HITO_FFMPEG", directory.path().join("missing-ffmpeg"));
    }
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let host = LocalHost::start(Paths {
        data: directory.path().join("data"),
        music,
        web,
    })
    .await
    .unwrap();
    let entry = host.entry_url();
    let root = entry.join("/").unwrap();
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let cookie = client.get(entry).send().await.unwrap().headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let call = |method: &'static str, path: String, token: String, body: Value| {
        let client = client.clone();
        let root = root.clone();
        let cookie = cookie.clone();
        async move {
            client
                .request(method.parse().unwrap(), root.join(&path).unwrap())
                .header("cookie", cookie)
                .header("x-hito-request", "1")
                .header("origin", root.origin().ascii_serialization())
                .header("authorization", format!("Bearer {token}"))
                .json(&body)
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap_or(Value::Null)
        }
    };
    let setup = call(
        "POST",
        "api/v1/setup".into(),
        String::new(),
        json!({"username":"owner","password":"local media tools test password"}),
    )
    .await;
    let token = setup["token"].as_str().unwrap().to_owned();
    let libraries = call("GET", "api/v1/libraries".into(), token.clone(), Value::Null).await;
    let library = libraries[0]["id"].as_str().unwrap().to_owned();
    let folder = call(
        "POST",
        format!("api/v1/libraries/{library}/folders"),
        token.clone(),
        json!({"directory": {"directory_id": "legacy-music", "components": []}}),
    )
    .await;
    assert!(folder["latest_job"].is_object(), "{folder}");
    let mut job = Value::Null;
    for _ in 0..100 {
        let folders = call(
            "GET",
            format!("api/v1/libraries/{library}/folders"),
            token.clone(),
            Value::Null,
        )
        .await;
        job = folders[0]["latest_job"].clone();
        if job["state"] == "failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(job["state"], "failed", "{job}");
    assert_eq!(job["failure"], "media_tools_missing");
    assert!(
        job["error"]
            .as_str()
            .unwrap()
            .contains("FFmpeg is not installed")
    );
    host.shutdown().await.unwrap();
}
