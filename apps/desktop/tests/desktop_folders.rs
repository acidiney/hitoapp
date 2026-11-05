use futures_util::future::BoxFuture;
use hito_desktop::{FolderPicker, HostChoice, LocalHost, Paths};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct Fake(Mutex<Option<PathBuf>>);

impl FolderPicker for Fake {
    fn pick(&self) -> BoxFuture<'_, HostChoice> {
        Box::pin(async {
            match self.0.lock().unwrap().take() {
                Some(path) => HostChoice::Chosen(path),
                None => HostChoice::Cancelled,
            }
        })
    }
}

struct Session {
    client: Client,
    root: url::Url,
    cookie: String,
    token: String,
}

impl Session {
    async fn call(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let response = self
            .client
            .request(method.parse().unwrap(), self.root.join(path).unwrap())
            .header("cookie", &self.cookie)
            .header("x-hito-request", "1")
            .header("origin", self.root.origin().ascii_serialization())
            .header("authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .await
            .unwrap();
        (
            response.status(),
            response.json().await.unwrap_or(Value::Null),
        )
    }
}

async fn start(
    music_exists: bool,
    chosen: Option<PathBuf>,
) -> (tempfile::TempDir, LocalHost, Session) {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    let music = directory.path().join("music");
    if music_exists {
        std::fs::create_dir(&music).unwrap();
    }
    let host = LocalHost::start_with_folders(
        Paths {
            data: directory.path().join("data"),
            music,
            web,
        },
        None,
        Some(Arc::new(Fake(Mutex::new(chosen)))),
        None,
    )
    .await
    .unwrap();
    let entry = host.entry_url();
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let bootstrap = client.get(entry.clone()).send().await.unwrap();
    let cookie = bootstrap.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let mut session = Session {
        client,
        root: entry.join("/").unwrap(),
        cookie,
        token: String::new(),
    };
    let (status, body) = session
        .call(
            "POST",
            "api/v1/setup",
            json!({"username":"owner","password":"local folder test password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    session.token = body["token"].as_str().unwrap().to_owned();
    (directory, host, session)
}

#[tokio::test]
async fn an_existing_music_folder_is_proposed_as_a_reference_and_not_connected() {
    let (_directory, host, session) = start(true, None).await;
    let (_, offer) = session
        .call("GET", "api/v1/host/folders", Value::Null)
        .await;
    assert_eq!(offer["picker"], true);
    assert_eq!(
        offer["suggestion"],
        json!({"directory_id":"legacy-music","components":[]})
    );
    let (_, libraries) = session.call("GET", "api/v1/libraries", Value::Null).await;
    let library = libraries[0]["id"].as_str().unwrap();
    let (_, folders) = session
        .call(
            "GET",
            &format!("api/v1/libraries/{library}/folders"),
            Value::Null,
        )
        .await;
    assert_eq!(folders, json!([]));
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_missing_music_folder_is_not_proposed() {
    let (_directory, host, session) = start(false, None).await;
    let (_, offer) = session
        .call("GET", "api/v1/host/folders", Value::Null)
        .await;
    assert_eq!(offer, json!({"picker": true, "suggestion": null}));
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_natively_chosen_folder_connects_through_the_existing_folder_route() {
    let elsewhere = tempfile::tempdir().unwrap();
    let (_directory, host, session) = start(true, Some(elsewhere.path().to_owned())).await;
    let (status, chosen) = session
        .call("POST", "api/v1/host/folders/choose", Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, libraries) = session.call("GET", "api/v1/libraries", Value::Null).await;
    let library = libraries[0]["id"].as_str().unwrap();
    let (status, folder) = session
        .call(
            "POST",
            &format!("api/v1/libraries/{library}/folders"),
            json!({"directory": chosen["directory"]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(
        !folder
            .to_string()
            .contains(elsewhere.path().to_str().unwrap())
    );
    let (_, again) = session
        .call("POST", "api/v1/host/folders/choose", Value::Null)
        .await;
    assert_eq!(again, json!({"directory": null}));
    host.shutdown().await.unwrap();
}
