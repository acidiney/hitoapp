use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::test_support::support;

use hito_core::{Action, Search};
use uuid::Uuid;

#[tokio::test(flavor = "current_thread")]
async fn history_workflows_log_outcomes_and_counts_without_titles_or_identifiers() {
    let (_dir, hito, actor, _library) = support::catalog().await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items;
    let captured = logs::CapturedLogs::default();
    tracing::subscriber::set_global_default(captured.subscriber()).unwrap();
    hito.collection
        .set_history_enabled(&actor, false)
        .await
        .unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::ProgressSet {
            recording_id: Uuid::parse_str(&recordings[0].id).unwrap(),
            position_ms: 10,
            speed: 1.0,
        }],
    )
    .await;
    hito.collection
        .set_history_enabled(&actor, true)
        .await
        .unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::ProgressSet {
            recording_id: Uuid::parse_str(&recordings[1].id).unwrap(),
            position_ms: 10,
            speed: 1.0,
        }],
    )
    .await;
    assert_eq!(
        hito.collection.clear_history(&actor).await.unwrap().removed,
        1
    );
    let text = captured.text();
    assert!(
        text.contains("Listening history preference changed"),
        "{text}"
    );
    assert!(text.contains("\"history_enabled\":false"), "{text}");
    assert!(text.contains("Listening history deleted"), "{text}");
    assert!(text.contains("\"removed\":1"), "{text}");
    assert!(text.contains("\"history_recorded\":false"), "{text}");
    assert!(text.contains("\"history_recorded\":true"), "{text}");
    for recording in &recordings {
        assert!(!text.contains(&recording.id), "{text}");
        assert!(!text.contains(&recording.title), "{text}");
    }
}
