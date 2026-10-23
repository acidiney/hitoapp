#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[path = "metadata/fixture.rs"]
mod fixture;

use fixture::{Fixture, recording, write};
use hito_plugin_sdk::{Capability, Manifest, ProviderRecording, SourcePage};
use serde_json::{Value, json};

const TITLE: &str = "Owned performer - Owned tune (Official Audio)";
const MUSICAL_TITLE: &str = "Owned tune";
const ALBUM: &str = "Live at the Owned test room";
const ARTIST: &str = "Owned performer, Owned guest";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn musical_metadata_retains_the_legacy_projection_and_its_source_snapshot() {
    let directory = tempfile::tempdir().unwrap_or_else(|_| panic!("metadata setup failed"));
    let mut detail = recording("HitoMeta001", TITLE);
    detail["track"] = json!(MUSICAL_TITLE);
    detail["artists"] = json!(["Owned performer", "Owned guest"]);
    detail["album"] = json!(ALBUM);
    detail["album_artists"] = json!(["Owned performer"]);
    write(directory.path(), "structured.json", &detail);
    let fixture = Fixture::start(directory).await;
    let result = verify_metadata(&fixture).await;
    fixture.finish(result, b"2\n").await;
}

async fn verify_metadata(fixture: &Fixture) -> Result<(), &'static str> {
    let request = json!({"recording_id":"HitoMeta001"});
    let (status, detail_bytes) = fixture
        .response("/v1/recordings/detail", Some(&request), "legacy_detail")
        .await?;
    if status != reqwest::StatusCode::OK {
        return Err("legacy_detail");
    }
    let detail: ProviderRecording =
        serde_json::from_slice(&detail_bytes).map_err(|_| "legacy_detail")?;
    detail.validate().map_err(|_| "legacy_detail")?;
    if detail.id != "HitoMeta001"
        || detail.title != TITLE
        || detail.artist != ARTIST
        || detail.duration_ms != Some(12_500)
        || detail.release.is_some()
        || detail.disc_number.is_some()
        || detail.track_number.is_some()
        || detail.revision.is_some()
    {
        return Err("legacy_detail");
    }
    let (status, source_bytes) = fixture
        .response("/v1/sources", Some(&request), "legacy_sources")
        .await?;
    if status != reqwest::StatusCode::OK {
        return Err("legacy_sources");
    }
    let sources: SourcePage =
        serde_json::from_slice(&source_bytes).map_err(|_| "legacy_sources")?;
    sources.validate().map_err(|_| "legacy_sources")?;
    if sources.items.len() != 1
        || !sources.items[0].ready
        || sources.items[0].label != "AAC / M4A"
        || !sources.items[0].id.starts_with("youtube:v1:HitoMeta001:")
    {
        return Err("legacy_sources");
    }
    let (status, body) = fixture
        .response(
            "/v1/recordings/metadata",
            Some(&request),
            "metadata_endpoint",
        )
        .await?;
    if status != reqwest::StatusCode::OK {
        return Err(if status == reqwest::StatusCode::NOT_FOUND {
            "metadata_endpoint_404"
        } else {
            "metadata_endpoint_status"
        });
    }
    let metadata: Value = serde_json::from_slice(&body).map_err(|_| "metadata_shape")?;
    let object = metadata.as_object().ok_or("metadata_shape")?;
    if object.len() != 4
        || metadata["musical_title"] != MUSICAL_TITLE
        || metadata["album_title"] != ALBUM
        || metadata["source_ids"] != json!([sources.items[0].id])
    {
        return Err("metadata_evidence");
    }
    let projection: ProviderRecording =
        serde_json::from_value(metadata["recording"].clone()).map_err(|_| "metadata_projection")?;
    projection.validate().map_err(|_| "metadata_projection")?;
    if projection != detail {
        return Err("metadata_projection");
    }
    let manifest: Manifest = fixture
        .json("/v1/manifest", None, "metadata_manifest")
        .await?;
    manifest.validate().map_err(|_| "metadata_manifest")?;
    if !manifest.capabilities.contains(&Capability::Metadata) {
        return Err("metadata_capability");
    }
    for (path, before) in [
        ("/v1/recordings/detail", &detail_bytes),
        ("/v1/sources", &source_bytes),
    ] {
        let (status, after) = fixture
            .response(path, Some(&request), "legacy_projection_unchanged")
            .await?;
        if status != reqwest::StatusCode::OK || after != *before {
            return Err("legacy_projection_unchanged");
        }
    }
    Ok(())
}
