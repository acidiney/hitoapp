use hito_core::test_support::{metadata_tag_logs as logs, playback as fixture, support};

#[tokio::test(flavor = "current_thread")]
async fn fallback_steps_log_their_outcome_but_never_sources_or_paths() {
    let f = fixture::LocalPlayback::start().await;
    for copy in 0..2 {
        std::fs::write(f.music.join(format!("copy-{copy}.wav")), &f.original).unwrap();
    }
    support::scan(&f.services, &f.actor, &f.library).await;
    let captured = logs::CapturedLogs::default();
    tracing::subscriber::set_global_default(captured.subscriber()).unwrap();
    let mut plan = f
        .services
        .playback
        .playback_candidates(&f.actor, &f.recording)
        .await
        .unwrap();
    let mut seen = Vec::new();
    while let Some(grant) = plan.next().await.unwrap() {
        seen.push(grant.source_id().to_owned());
    }
    assert_eq!(seen.len(), 3);
    let preference = f
        .services
        .catalog
        .source_preferences(&f.actor)
        .await
        .unwrap();
    f.services
        .catalog
        .update_source_preferences(
            &f.actor,
            hito_core::UpdateSourcePreferences {
                revision: preference.revision,
                fallback: false,
                entries: preference.entries,
            },
        )
        .await
        .unwrap();
    let text = captured.text();
    assert!(
        text.contains("\"event\":\"playback_source_fallback\""),
        "{text}"
    );
    assert!(text.contains("\"outcome\":\"next_source\""), "{text}");
    assert!(text.contains("\"outcome\":\"exhausted\""), "{text}");
    assert!(
        text.contains("source_preferences_update_finished"),
        "{text}"
    );
    assert!(text.contains("\"outcome\":\"saved\""), "{text}");
    for source in &seen {
        assert!(!text.contains(source.as_str()), "{text}");
    }
    assert!(!text.contains(f.music.to_str().unwrap()), "{text}");
    assert!(!text.contains("copy-0.wav"), "{text}");
}
