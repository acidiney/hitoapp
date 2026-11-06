use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;
use std::collections::BTreeSet;

#[tokio::test]
async fn candidates_visit_each_copy_once_and_new_copies_require_a_new_plan() {
    let f = fixture::LocalPlayback::start().await;
    std::fs::write(f.music.join("copy.wav"), &f.original).unwrap();
    support::scan(&f.services, &f.actor, &f.library).await;
    let expected = f
        .services
        .catalog
        .sources(&f.actor, &f.recording)
        .await
        .unwrap()
        .into_iter()
        .map(|source| source.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), 2);
    let mut plan = f
        .services
        .playback
        .playback_candidates(&f.actor, &f.recording)
        .await
        .unwrap();
    let mut seen = BTreeSet::new();
    while let Some(grant) = plan.next().await.unwrap() {
        assert!(
            seen.insert(grant.source_id().to_owned()),
            "a candidate repeated"
        );
        assert_eq!(
            support::audio_bytes(grant.open_file().await.unwrap()),
            f.original
        );
    }
    assert_eq!(seen, expected);
    std::fs::write(f.music.join("later.wav"), &f.original).unwrap();
    support::scan(&f.services, &f.actor, &f.library).await;
    assert!(plan.next().await.unwrap().is_none());
    let mut fresh = f
        .services
        .playback
        .playback_candidates(&f.actor, &f.recording)
        .await
        .unwrap();
    let mut count = 0;
    while fresh.next().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 3);
    f.services.identity.logout(&f.token).await.unwrap();
    assert!(matches!(
        fresh.next().await,
        Err(hito_core::Error::Unauthorized)
    ));
}

#[tokio::test]
async fn fallback_attempts_are_bounded_and_the_stop_is_recorded() {
    let f = fixture::LocalPlayback::start().await;
    for copy in 0..9 {
        std::fs::write(f.music.join(format!("copy-{copy}.wav")), &f.original).unwrap();
    }
    support::scan(&f.services, &f.actor, &f.library).await;
    let mut plan = f
        .services
        .playback
        .playback_candidates(&f.actor, &f.recording)
        .await
        .unwrap();
    let mut offered = 0;
    while plan.next().await.unwrap().is_some() {
        offered += 1;
        assert!(offered <= 100, "candidate iteration must terminate");
    }
    assert_eq!(offered, hito_core::MAX_FALLBACK_ATTEMPTS);
    assert_eq!(plan.ended(), Some(hito_core::CandidatesEnd::AttemptLimit));
    assert!(plan.next().await.unwrap().is_none());
    assert_eq!(plan.ended(), Some(hito_core::CandidatesEnd::AttemptLimit));
}

#[tokio::test]
async fn running_out_of_candidates_is_recorded_as_exhaustion() {
    let f = fixture::LocalPlayback::start().await;
    let mut plan = f
        .services
        .playback
        .playback_candidates(&f.actor, &f.recording)
        .await
        .unwrap();
    assert!(plan.next().await.unwrap().is_some());
    assert!(plan.ended().is_none());
    assert!(plan.next().await.unwrap().is_none());
    assert_eq!(plan.ended(), Some(hito_core::CandidatesEnd::Exhausted));
}
