#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[path = "metadata/fixture.rs"]
mod fixture;

use fixture::{Fixture, recording, write};
use hito_plugin_sdk::{Capability, Manifest, RecordingMetadata};
use serde_json::json;

const OWN_LINKS: [(&str, &str); 5] = [
    (
        "HitoCheck01",
        "https://www.youtube.com/watch?v=HitoCheck01&t=7s",
    ),
    (
        "HitoCheck02",
        "https://music.youtube.com/watch?v=HitoCheck02&si=private-share-sentinel",
    ),
    (
        "HitoCheck03",
        "https://youtu.be/HitoCheck03?si=private-share-sentinel",
    ),
    ("HitoCheck04", "https://www.youtube.com/shorts/HitoCheck04"),
    ("HitoCheck05", "HitoCheck05"),
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lookup_answers_own_links_through_the_canonical_extraction_only() {
    let directory = tempfile::tempdir().unwrap_or_else(|_| panic!("lookup setup failed"));
    for (id, _) in OWN_LINKS {
        write(
            directory.path(),
            &format!("{id}.json"),
            &recording(id, "Owned linked title"),
        );
    }
    write(
        directory.path(),
        "HitoCheck06.json",
        &recording("HitoCheck06", "Owned foreign title"),
    );
    let fixture = Fixture::start(directory).await;
    let result = verify_lookup(&fixture).await;
    fixture.finish(result, b"4\n4\n4\n4\n4\n").await;
}

async fn verify_lookup(fixture: &Fixture) -> Result<(), &'static str> {
    let manifest: Manifest = fixture
        .json("/v1/manifest", None, "lookup_manifest")
        .await?;
    manifest.validate().map_err(|_| "lookup_manifest")?;
    if !manifest.capabilities.contains(&Capability::SourceLinks)
        || !manifest.capabilities.contains(&Capability::Sources)
    {
        return Err("lookup_capability");
    }
    for (id, reference) in OWN_LINKS {
        let metadata: RecordingMetadata = fixture
            .json(
                "/v1/recordings/lookup",
                Some(&json!({"reference": reference})),
                "lookup_own_link",
            )
            .await?;
        metadata.validate().map_err(|_| "lookup_shape")?;
        if metadata.recording.id != id
            || metadata
                .source_ids
                .as_ref()
                .is_none_or(|sources| sources.len() != 1)
        {
            return Err("lookup_identity");
        }
    }
    let overlong = format!(
        "https://www.youtube.com/watch?v=HitoCheck06&pad={}",
        "a".repeat(2048)
    );
    for (reference, expected) in [
        (
            "https://evil.example/watch?v=HitoCheck06",
            reqwest::StatusCode::NOT_FOUND,
        ),
        (
            "https://www.youtube.com.evil.example/watch?v=HitoCheck06",
            reqwest::StatusCode::NOT_FOUND,
        ),
        (
            "https://user@www.youtube.com/watch?v=HitoCheck06",
            reqwest::StatusCode::NOT_FOUND,
        ),
        (
            "https://www.youtube.com:8443/watch?v=HitoCheck06",
            reqwest::StatusCode::NOT_FOUND,
        ),
        (
            "https://www.youtube.com/watch?v=HitoCheck6",
            reqwest::StatusCode::NOT_FOUND,
        ),
        (
            "https://www.youtube.com/playlist?list=HitoCheck06",
            reqwest::StatusCode::NOT_FOUND,
        ),
        (
            "https://www.youtube.com/watch?v=HitoCheck06\n",
            reqwest::StatusCode::BAD_REQUEST,
        ),
        (overlong.as_str(), reqwest::StatusCode::BAD_REQUEST),
        ("", reqwest::StatusCode::BAD_REQUEST),
    ] {
        let (status, _) = fixture
            .response(
                "/v1/recordings/lookup",
                Some(&json!({"reference": reference})),
                "lookup_rejection",
            )
            .await?;
        if status != expected {
            return Err("lookup_rejection_status");
        }
    }
    for malformed in [
        json!({}),
        json!({"reference": 7}),
        json!({"reference": "HitoCheck06", "url": "https://www.youtube.com/watch?v=HitoCheck06"}),
    ] {
        let (status, _) = fixture
            .response(
                "/v1/recordings/lookup",
                Some(&malformed),
                "lookup_malformed",
            )
            .await?;
        if status != reqwest::StatusCode::BAD_REQUEST {
            return Err("lookup_malformed_status");
        }
    }
    Ok(())
}
