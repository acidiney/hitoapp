use hito_desktop::{LocalHost, Paths};
use std::{
    io::Write,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn rejected_bootstrap_input_never_appears_in_trace_logs_or_responses() {
    let logs = Capture::default();
    let writer = logs.clone();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .try_init()
        .unwrap();
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
    let mut entry = host.entry_url();
    let ticket = entry.query_pairs().next().unwrap().1.into_owned();
    let sentinel = "PRIVATE_BOOTSTRAP_QUERY_SENTINEL";
    entry.query_pairs_mut().append_pair(sentinel, "rejected");
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
        .get(entry)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    host.shutdown().await.unwrap();
    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    for private in [&ticket, sentinel] {
        assert!(
            !body.contains(private),
            "Rejected private input appeared in the response"
        );
        assert!(
            !output.contains(private),
            "Rejected private input appeared in TRACE logs"
        );
    }
    assert!(output.contains("desktop_bootstrap_rejected"));
}
