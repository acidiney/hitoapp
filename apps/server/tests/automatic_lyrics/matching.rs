use super::setup::{Prepared, commit_metadata};
use crate::provider;
use serde_json::json;

#[tokio::test]
async fn automatic_matching_refuses_unexamined_continuations_without_contacting_candidates() {
    let prepared = Prepared::new().await;
    let before = prepared.library.snapshot().await;
    prepared.provider.reply(
        "search",
        json!({"items":[provider::recording()],"next_cursor":"owned:next-page"}),
    );
    assert_eq!(prepared.read().await, json!({"status":"ambiguous"}));
    assert_eq!(prepared.provider.calls(), vec!["manifest", "search"]);
    assert_eq!(prepared.library.snapshot().await, before);
    prepared.finish().await;
}

#[tokio::test]
async fn automatic_matching_keeps_versions_featured_artists_and_duration_evidence_distinct() {
    let prepared = Prepared::new().await;
    let before = prepared.library.snapshot().await;
    let mut candidates = Vec::new();
    for suffix in [
        " (Live)",
        " (Acoustic)",
        " (Owned Remix)",
        " (2011 Remaster)",
        " (Instrumental)",
    ] {
        let mut candidate = provider::recording();
        candidate["title"] = json!(format!("{}{suffix}", crate::lrclib::TITLE));
        candidates.push(candidate);
    }
    let mut featured = provider::recording();
    featured["artist"] = json!(format!("{} feat. Owned guest", crate::lrclib::ARTIST));
    candidates.push(featured);
    for duration in [json!(null), json!(0), json!(5000)] {
        let mut candidate = provider::recording();
        candidate["duration_ms"] = duration;
        candidates.push(candidate);
    }
    let mut released = provider::recording();
    released["release"] =
        json!({"id":"owned:remix-release","title":"Owned Remix Edition","artist":null});
    candidates.push(released);
    for candidate in candidates {
        prepared
            .provider
            .reply("search", json!({"items":[candidate],"next_cursor":null}));
        assert_eq!(prepared.read().await, json!({"status":"not_found"}));
    }
    prepared.provider.reply(
        "search",
        json!({"items":[provider::recording()],"next_cursor":null}),
    );
    for album in [
        "Owned Live at venue",
        "Owned Remix",
        "Owned Acoustic",
        "Owned 2011 Remastered",
        "Owned Unplugged",
        "Owned Radio Edit",
        "Owned Instrumental",
    ] {
        prepared.provider.reply(
            "metadata",
            json!({"recording":provider::recording(),"album_title":album}),
        );
        assert_eq!(prepared.read().await, json!({"status":"not_found"}));
    }
    assert!(
        !prepared
            .provider
            .calls()
            .iter()
            .any(|call| call.contains("descriptor") || call.contains("lyrics"))
    );
    assert_eq!(prepared.library.snapshot().await, before);
    prepared.finish().await;
}

#[tokio::test]
async fn automatic_matching_rejects_a_disproportionately_longer_short_recording() {
    let prepared = Prepared::new().await;
    let mut longer = provider::recording();
    longer["duration_ms"] = json!(3000);
    prepared.provider.reply(
        "search",
        json!({"items":[longer.clone()],"next_cursor":null}),
    );
    prepared
        .provider
        .reply("metadata", json!({"recording":longer}));
    assert_eq!(
        prepared.read().await,
        json!({"status":"not_found"}),
        "The 2s absolute cap must not admit a recording three times the local duration"
    );
    prepared.finish().await;
}

#[tokio::test]
async fn duration_confidence_uses_real_scanned_audio_at_exact_and_just_beyond_absolute_and_relative_boundaries()
 {
    for (seconds, candidates) in [
        (
            120,
            vec![
                (122000, true),
                (122001, false),
                (118000, true),
                (117999, false),
            ],
        ),
        (
            40,
            vec![(42000, true), (42001, false), (38096, true), (38095, false)],
        ),
        (1, vec![(1050, true), (1051, false)]),
    ] {
        let prepared = Prepared::with_duration(seconds).await;
        let recordings = prepared
            .library
            .account
            .get(&prepared.library.app, "/api/v1/recordings")
            .await;
        assert_eq!(recordings["items"][0]["duration_ms"], seconds * 1000);
        let before = prepared.library.snapshot().await;
        for (duration, expected_match) in candidates {
            let mut candidate = provider::recording();
            candidate["duration_ms"] = json!(duration);
            prepared.provider.reply(
                "search",
                json!({"items":[candidate.clone()],"next_cursor":null}),
            );
            prepared
                .provider
                .reply("metadata", json!({"recording":candidate}));
            let response = prepared.read().await;
            assert_eq!(
                response["status"],
                if expected_match {
                    "ready_v2"
                } else {
                    "not_found"
                }
            );
        }
        assert_eq!(prepared.library.snapshot().await, before);
        prepared.finish().await;
    }
}

#[tokio::test]
async fn contradictory_musical_title_metadata_cannot_override_the_committed_recording_version() {
    let prepared = Prepared::new().await;
    prepared.provider.reply("metadata", json!({"recording":provider::recording(),"musical_title":format!("{} (Live)", crate::lrclib::TITLE)}));
    assert_eq!(prepared.read().await, json!({"status":"not_found"}));
    assert_eq!(
        prepared.provider.calls(),
        vec!["manifest", "search", "metadata"]
    );
    prepared.finish().await;
}

#[tokio::test]
async fn one_exact_version_and_featured_artist_match_is_selected_even_after_a_higher_ranked_wrong_recording()
 {
    let prepared = Prepared::new().await;
    let title = format!("{} (Live)", crate::lrclib::TITLE);
    let artist = format!("{} feat. Owned guest", crate::lrclib::ARTIST);
    commit_metadata(&prepared.library, &title, &artist).await;
    prepared.provider.query(format!("{artist} {title}"));
    let mut matching = provider::recording();
    matching["title"] = json!(title);
    matching["artist"] = json!(artist);
    let mut wrong = provider::recording();
    wrong["id"] = json!("owned:higher-ranked-other-version");
    prepared.provider.reply(
        "search",
        json!({"items":[wrong,matching.clone()],"next_cursor":null}),
    );
    prepared.provider.reply(
        "metadata",
        json!({"recording":matching,"musical_title":title}),
    );
    let before = prepared.library.snapshot().await;
    let response = prepared.read().await;
    assert_eq!(response["status"], "ready_v2");
    assert_eq!(response["provider_recording"], provider::reference());
    assert_eq!(
        prepared.provider.calls(),
        vec![
            "manifest",
            "search",
            "metadata",
            "v2_descriptor",
            "v2_lyrics"
        ]
    );
    assert_eq!(prepared.library.snapshot().await, before);
    prepared.finish().await;
}
