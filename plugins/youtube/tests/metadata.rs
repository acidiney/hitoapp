#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[path = "metadata/fixture.rs"]
mod fixture;

use fixture::{Fixture, recording, write};
use hito_plugin_sdk::{Manifest, ProviderRecording, SearchPage, SourcePage};
use serde_json::{Value, json};

const TITLE: &str = "Owned tune (Live at the test room)";
const CHANNEL_TITLE: &str = "Owned tune (Official Remix)";
const ARTISTS: &str = "Owned performer, Owned guest";
const CHANNEL: &str = "Channel: Owned Official channel";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_music_artists_survive_detail_without_erasing_video_versions() {
    let directory = tempfile::tempdir().unwrap_or_else(|_| panic!("metadata setup failed"));
    let mut structured = recording("HitoMeta001", TITLE);
    structured["track"] = json!("Owned tune");
    structured["artists"] = json!(["Owned performer", "Owned guest"]);
    structured["artist"] = json!("Legacy incomplete performer");
    structured["album"] = json!("Owned live album");
    structured["album_artists"] = json!(["Owned performer"]);
    let channel = recording("HitoMeta002", CHANNEL_TITLE);
    let search = json!({"entries":[
        {"id":"HitoMeta001","title":TITLE,"channel":"Owned Official channel","duration":12.5},
        {"id":"HitoMeta002","title":CHANNEL_TITLE,"channel":"Owned Official channel","duration":12.5}
    ]});
    for (name, value) in [
        ("search.json", search),
        ("structured.json", structured),
        ("channel.json", channel),
    ] {
        write(directory.path(), name, &value);
    }
    let fixture = Fixture::start(directory).await;
    let result = verify(&fixture).await;
    fixture.finish(result, b"1\n2\n3\n").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_musical_artists_are_rejected_without_losing_legacy_compatibility() {
    let directory = tempfile::tempdir().unwrap_or_else(|_| panic!("metadata setup failed"));
    let cases = [
        (
            Some(json!([" "])),
            Some("Legacy performer"),
            None,
            "blank_artist",
        ),
        (
            Some(json!(["Owned performer", ""])),
            Some("Legacy performer"),
            None,
            "blank_member",
        ),
        (
            Some(json!(["Owned\nperformer"])),
            Some("Legacy performer"),
            None,
            "control_member",
        ),
        (
            Some(json!([7])),
            Some("Legacy performer"),
            None,
            "numeric_member",
        ),
        (
            Some(json!([null])),
            Some("Legacy performer"),
            None,
            "null_member",
        ),
        (
            Some(json!(["é".repeat(512), "é".repeat(512)])),
            Some("Legacy performer"),
            None,
            "oversized_joined_artist",
        ),
        (
            Some(json!("Owned performer")),
            Some("Legacy performer"),
            None,
            "nonlist_artists",
        ),
        (
            Some(json!([])),
            Some("Legacy performer"),
            Some("Legacy performer"),
            "empty_list_legacy",
        ),
        (
            None,
            Some("Legacy performer"),
            Some("Legacy performer"),
            "absent_list_legacy",
        ),
        (Some(json!([])), None, Some(CHANNEL), "empty_list_channel"),
        (None, None, Some(CHANNEL), "absent_list_channel"),
        (
            Some(Value::Null),
            Some("Legacy performer"),
            Some("Legacy performer"),
            "null_list_legacy",
        ),
    ];
    let mut expected = Vec::with_capacity(cases.len());
    for (index, (artists, legacy, result, phase)) in cases.into_iter().enumerate() {
        let id = format!("HitoCheck{index:02}");
        let mut detail = recording(&id, TITLE);
        if let Some(artists) = artists {
            detail["artists"] = artists;
        }
        if let Some(legacy) = legacy {
            detail["artist"] = json!(legacy);
        }
        write(directory.path(), &format!("{id}.json"), &detail);
        expected.push((id, result, phase));
    }
    let fixture = Fixture::start(directory).await;
    let result = verify_validation(&fixture, &expected).await;
    fixture.finish(result, &b"4\n".repeat(expected.len())).await;
}

async fn verify(fixture: &Fixture) -> Result<(), &'static str> {
    let manifest: Manifest = fixture.json("/v1/manifest", None, "manifest").await?;
    manifest.validate().map_err(|_| "manifest")?;
    let page: SearchPage = fixture
        .json(
            "/v1/search",
            Some(&json!({"query":"private-search-query-sentinel","cursor":null,"limit":2})),
            "flat_search",
        )
        .await?;
    page.validate(2).map_err(|_| "flat_search")?;
    if page.items.len() != 2
        || page.next_cursor.is_some()
        || page.items[0].id != "HitoMeta001"
        || page.items[1].id != "HitoMeta002"
        || page
            .items
            .iter()
            .any(|item| item.artist != CHANNEL || item.release.is_some())
        || page.items[0].title != TITLE
        || page.items[1].title != CHANNEL_TITLE
    {
        return Err("flat_search");
    }
    for (id, title, artist, phase) in [
        ("HitoMeta001", TITLE, ARTISTS, "structured_detail"),
        ("HitoMeta002", CHANNEL_TITLE, CHANNEL, "channel_detail"),
    ] {
        let payload = json!({"recording_id":id});
        let detail: ProviderRecording = fixture
            .json("/v1/recordings/detail", Some(&payload), phase)
            .await?;
        detail.validate().map_err(|_| phase)?;
        if detail.id != id
            || detail.title != title
            || detail.artist != artist
            || detail.duration_ms != Some(12_500)
            || detail.release.is_some()
        {
            return Err(phase);
        }
        let mut source_id = None;
        for _ in 0..2 {
            let sources: SourcePage = fixture
                .json("/v1/sources", Some(&payload), "source_identity")
                .await?;
            sources.validate().map_err(|_| "source_identity")?;
            if sources.items.len() != 1
                || !sources.items[0].ready
                || sources.items[0].label != "AAC / M4A"
                || !sources.items[0]
                    .id
                    .starts_with(&format!("youtube:v1:{id}:"))
                || source_id
                    .as_ref()
                    .is_some_and(|previous| previous != &sources.items[0].id)
            {
                return Err("source_identity");
            }
            source_id = Some(sources.items[0].id.clone());
        }
    }
    Ok(())
}

async fn verify_validation(
    fixture: &Fixture,
    cases: &[(String, Option<&str>, &'static str)],
) -> Result<(), &'static str> {
    for (id, artist, phase) in cases {
        let (status, body) = fixture
            .response(
                "/v1/recordings/detail",
                Some(&json!({"recording_id":id})),
                phase,
            )
            .await?;
        if let Some(artist) = artist {
            if status != reqwest::StatusCode::OK {
                return Err(phase);
            }
            let detail: ProviderRecording = serde_json::from_slice(&body).map_err(|_| *phase)?;
            detail.validate().map_err(|_| *phase)?;
            if detail.id != *id
                || detail.title != TITLE
                || detail.artist != *artist
                || detail.duration_ms != Some(12_500)
                || detail.release.is_some()
            {
                return Err(phase);
            }
        } else {
            let error: Value = serde_json::from_slice(&body).map_err(|_| *phase)?;
            if status != reqwest::StatusCode::BAD_GATEWAY
                || error != json!({"error":"YouTube returned an invalid search response"})
            {
                return Err(phase);
            }
        }
    }
    Ok(())
}
