use super::{
    setup::Prepared,
    timed_priority::{candidate, content},
};
use crate::provider;
use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn automatic_read_keeps_provider_order_within_timed_and_plain_tiers_and_examines_plain_fallbacks()
 {
    for (kinds, expected, visited) in [
        (vec![Some(false), Some(false)], Some(0), 2),
        (vec![Some(true), Some(true)], Some(0), 1),
        (vec![Some(false), None, Some(true)], Some(2), 3),
        (vec![None, None], None, 2),
        (vec![Some(false); 20], Some(0), 20),
    ] {
        let prepared = Prepared::new().await;
        let ids: Vec<_> = (0..kinds.len())
            .map(|index| format!("owned:tier-{index}"))
            .collect();
        let candidates: Vec<_> = ids
            .iter()
            .zip(kinds)
            .map(|(id, kind)| candidate(&prepared.provider, id, kind.map(content)))
            .collect();
        prepared
            .provider
            .reply("search", json!({"items":candidates,"next_cursor":null}));
        let before = prepared.library.snapshot().await;
        let response = prepared.read().await;
        assert_eq!(prepared.library.snapshot().await, before);
        assert_eq!(
            prepared.provider.requests_for("metadata"),
            ids[..visited]
                .iter()
                .map(|id| json!({"recording_id":id}))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            prepared.provider.requests_for("v2_lyrics"),
            ids[..visited]
                .iter()
                .map(|id| json!({"recording":{"id":id,"revision":"owned:1"}}))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            prepared
                .provider
                .calls()
                .iter()
                .filter(|operation| **operation == "v2_descriptor")
                .count(),
            1
        );
        prepared.finish().await;
        if let Some(index) = expected {
            assert_eq!(response["status"], "ready_v2");
            assert_eq!(response["provider_recording"]["id"], ids[index]);
        } else {
            assert_eq!(response, json!({"status":"not_found"}));
        }
    }
}

#[tokio::test]
async fn timed_preference_preserves_summary_and_refetched_musical_confidence() {
    for mismatch in ["title", "artist", "duration", "metadata_version"] {
        let prepared = Prepared::new().await;
        let plain = candidate(&prepared.provider, "owned:plain", Some(content(false)));
        let mut wrong = candidate(&prepared.provider, "owned:wrong-timed", Some(content(true)));
        let right = candidate(&prepared.provider, "owned:right-timed", Some(content(true)));
        match mismatch {
            "title" => wrong["title"] = json!("Owned different title"),
            "artist" => wrong["artist"] = json!("Owned different artist"),
            "duration" => wrong["duration_ms"] = json!(3000),
            "metadata_version" => prepared.provider.reply_for(
                "metadata",
                json!({"recording_id":"owned:wrong-timed"}),
                json!({"recording":wrong,"album_title":"Owned Live at venue"}),
            ),
            _ => unreachable!(),
        }
        prepared.provider.reply(
            "search",
            json!({"items":[plain,wrong,right],"next_cursor":null}),
        );
        let response = prepared.read().await;
        let metadata = prepared.provider.requests_for("metadata");
        let lyrics = prepared.provider.requests_for("v2_lyrics");
        prepared.finish().await;
        assert_eq!(response["provider_recording"]["id"], "owned:right-timed");
        assert_eq!(
            metadata.len(),
            if mismatch == "metadata_version" { 3 } else { 2 }
        );
        assert_eq!(
            lyrics,
            vec![
                json!({"recording":{"id":"owned:plain","revision":"owned:1"}}),
                json!({"recording":{"id":"owned:right-timed","revision":"owned:1"}})
            ]
        );
    }
}

#[tokio::test]
async fn a_plain_fallback_never_hides_a_failed_or_invalid_later_compatible_candidate() {
    for failure in [
        "metadata_status",
        "lyrics_status",
        "wrong_identity",
        "malformed_timing",
    ] {
        let prepared = Prepared::new().await;
        let plain = candidate(&prepared.provider, "owned:plain", Some(content(false)));
        let timed = candidate(&prepared.provider, "owned:later", Some(content(true)));
        let reference = json!({"recording":{"id":"owned:later","revision":"owned:1"}});
        match failure {
            "metadata_status" => prepared.provider.status_for(
                "metadata",
                json!({"recording_id":"owned:later"}),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            "lyrics_status" => prepared.provider.status_for(
                "v2_lyrics",
                reference,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            "wrong_identity" => {
                prepared
                    .provider
                    .reply_for("v2_lyrics", reference, provider::response())
            }
            "malformed_timing" => {
                let mut document = provider::response();
                document["recording"] = json!({"id":"owned:later","revision":"owned:1"});
                document["lyrics"]["content"] = json!({"kind":"anchored","lines":[{"start_ms":999,"text":"Owned later"},{"start_ms":1,"text":"Owned earlier"}]});
                prepared
                    .provider
                    .reply_for("v2_lyrics", reference, document);
            }
            _ => unreachable!(),
        }
        prepared
            .provider
            .reply("search", json!({"items":[plain,timed],"next_cursor":null}));
        let before = prepared.library.snapshot().await;
        let response = prepared.read().await;
        assert_eq!(prepared.library.snapshot().await, before);
        assert_eq!(prepared.provider.requests_for("metadata").len(), 2);
        assert_eq!(
            prepared.provider.requests_for("v2_lyrics").len(),
            if failure == "metadata_status" { 1 } else { 2 }
        );
        prepared.finish().await;
        assert_eq!(response, json!({"status":"unavailable"}));
    }
}
