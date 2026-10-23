use std::path::Path;

pub fn write_wav(path: &Path, title: Option<&str>, frequency: u32) -> Vec<u8> {
    write_tagged_wav(
        path,
        title,
        frequency,
        "Hito synthetic demo",
        "Owned test signals",
    )
}
pub fn write_tagged_wav(
    path: &Path,
    title: Option<&str>,
    frequency: u32,
    artist: &str,
    album: &str,
) -> Vec<u8> {
    let mut chunks = Vec::new();
    let mut format = Vec::new();
    format.extend(1u16.to_le_bytes());
    format.extend(1u16.to_le_bytes());
    format.extend(8000u32.to_le_bytes());
    format.extend(16000u32.to_le_bytes());
    format.extend(2u16.to_le_bytes());
    format.extend(16u16.to_le_bytes());
    chunk(&mut chunks, b"fmt ", &format);
    let audio: Vec<u8> = (0..160000)
        .flat_map(|index| {
            let phase = index as f64 * frequency as f64 * std::f64::consts::TAU / 8000.0;
            ((phase.sin() * 4000.0) as i16).to_le_bytes()
        })
        .collect();
    chunk(&mut chunks, b"data", &audio);
    if let Some(title) = title {
        let mut info = b"INFO".to_vec();
        for (key, value) in [(b"INAM", title), (b"IART", artist), (b"IPRD", album)] {
            chunk(&mut info, key, format!("{value}\0").as_bytes());
        }
        chunk(&mut chunks, b"LIST", &info);
    }
    let mut bytes = b"RIFF".to_vec();
    bytes.extend(((chunks.len() + 4) as u32).to_le_bytes());
    bytes.extend(b"WAVE");
    bytes.extend(chunks);
    std::fs::write(path, &bytes).unwrap();
    bytes
}
fn chunk(bytes: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
    bytes.extend(name);
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(data);
    if data.len() % 2 == 1 {
        bytes.push(0);
    }
}

pub async fn post(
    app: &axum::Router,
    path: &str,
    body: serde_json::Value,
) -> axum::response::Response {
    use tower::ServiceExt;
    app.clone()
        .oneshot(
            axum::http::Request::post(path)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
pub async fn search(
    app: &axum::Router,
    query: &str,
    cursor: Option<&str>,
    limit: u16,
) -> hito_plugin_sdk::SearchPage {
    use http_body_util::BodyExt;
    let response = post(
        app,
        "/v1/search",
        serde_json::json!({"query":query,"cursor":cursor,"limit":limit}),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
