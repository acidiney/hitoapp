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

mod manual_choice {
    use hito_core::test_support::provider_audio::{Alternate, AudioLibrary};
    use hito_core::{CandidatesEnd, ChooseSource};
    use std::{collections::BTreeSet, future::pending};
    use uuid::Uuid;

    struct World {
        f: AudioLibrary,
        alternate: Alternate,
    }

    async fn world() -> World {
        let f = AudioLibrary::start().await;
        let alternate = f.save_copy_and_attach_alternate().await;
        World { f, alternate }
    }

    impl World {
        fn first(&self) -> String {
            self.f.source.id.clone()
        }

        fn saved(&self) -> String {
            self.alternate.saved_copy.clone()
        }

        fn second(&self) -> String {
            self.alternate.second.clone()
        }

        async fn choose(&self, source: &str) {
            self.f
                .hito
                .catalog
                .choose_source(
                    &self.f.actor,
                    &self.f.source.recording_id,
                    ChooseSource {
                        request_id: Uuid::new_v4(),
                        source_id: source.parse().unwrap(),
                    },
                )
                .await
                .unwrap();
        }

        async fn native(&self) -> Vec<String> {
            let mut candidates = self
                .f
                .hito
                .playback
                .playback_candidates(&self.f.actor, &self.f.source.recording_id)
                .await
                .unwrap();
            let mut seen = Vec::new();
            while let Some(grant) = candidates.next().await.unwrap() {
                seen.push(grant.source_id().to_owned());
            }
            seen
        }

        async fn browser(&self) -> BTreeSet<String> {
            self.f
                .hito
                .catalog
                .playback_sources_until(&self.f.actor, &self.f.source.recording_id, pending())
                .await
                .unwrap()
                .into_iter()
                .filter(|source| source.ready)
                .map(|source| source.id)
                .collect()
        }
    }

    fn set(ids: &[String]) -> BTreeSet<String> {
        ids.iter().cloned().collect()
    }

    #[tokio::test]
    async fn a_choice_excludes_other_providers_and_their_saved_copies_like_the_browser() {
        let w = world().await;
        let all = set(&[w.saved(), w.first(), w.second()]);
        assert_eq!(set(&w.native().await), all);
        assert_eq!(w.browser().await, all);
        w.choose(&w.second()).await;
        assert_eq!(w.native().await, [w.second()]);
        assert_eq!(w.browser().await, set(&[w.second()]));
        w.choose(&w.first()).await;
        let kept = set(&[w.saved(), w.first()]);
        let native = w.native().await;
        assert_eq!(native.len(), 2);
        assert_eq!(set(&native), kept);
        assert_eq!(w.browser().await, kept);
    }

    #[tokio::test]
    async fn a_chosen_source_that_stops_working_never_falls_back_to_another_provider() {
        let w = world().await;
        w.choose(&w.second()).await;
        let mut candidates =
            w.f.hito
                .playback
                .playback_candidates(&w.f.actor, &w.f.source.recording_id)
                .await
                .unwrap();
        assert_eq!(
            candidates.next().await.unwrap().unwrap().source_id(),
            w.second()
        );
        assert!(candidates.next().await.unwrap().is_none());
        assert_eq!(candidates.ended(), Some(CandidatesEnd::Exhausted));
    }
}
