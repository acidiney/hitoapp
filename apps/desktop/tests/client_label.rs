use hito_desktop::{LocalHost, Paths, user_agent};
use reqwest::Client;
use serde_json::{Value, json};

#[tokio::test]
async fn a_desktop_sign_in_is_labelled_as_the_desktop_app_with_its_version() {
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
    let agent = user_agent();
    assert!(agent.ends_with(&format!("HitoDesktop/{}", env!("CARGO_PKG_VERSION"))));
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(agent)
        .build()
        .unwrap();
    let bootstrap = client.get(host.entry_url()).send().await.unwrap();
    let cookie = bootstrap.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let root = host.entry_url().join("/").unwrap();
    let call = |method: &'static str, path: &'static str, token: Option<String>, body: Value| {
        let mut request = client
            .request(method.parse().unwrap(), root.join(path).unwrap())
            .header("cookie", &cookie)
            .header("origin", root.origin().ascii_serialization())
            .header("x-hito-request", "1")
            .json(&body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        async move { request.send().await.unwrap().json::<Value>().await.unwrap() }
    };
    let setup = call(
        "POST",
        "api/v1/setup",
        None,
        json!({"username":"listener", "password":"private native desktop password"}),
    )
    .await;
    let token = setup["token"].as_str().unwrap().to_owned();
    let login = call(
        "POST",
        "api/v1/login",
        None,
        json!({"username":"listener", "password":"private native desktop password"}),
    )
    .await;
    let second = login["token"].as_str().unwrap().to_owned();
    let sessions = call("GET", "api/v1/account/sessions", Some(token), Value::Null).await;
    let clients: Vec<&str> = sessions
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["client"].as_str().unwrap())
        .collect();
    assert_eq!(clients, ["Hito desktop", "Hito desktop"]);
    assert!(!second.is_empty());
    host.shutdown().await.unwrap();
}

#[test]
fn the_desktop_agent_appends_the_token_to_a_platform_agent() {
    let agent = user_agent();
    assert!(agent.starts_with("Mozilla/5.0 ("));
    assert_eq!(agent.matches("HitoDesktop/").count(), 1);
    assert!(!agent.contains(char::is_control));
}
