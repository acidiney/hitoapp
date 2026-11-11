use super::*;
use crate::{capture::Monitor, measurement::Measurements, workflow::tests::PublicControl};
use hito_core::Credentials;
use hito_playback::Player;
use std::process::Command;
use uuid::Uuid;

#[tokio::test]
async fn startup_error_retires_real_core_resources_before_returning() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let weak = Arc::downgrade(&directory);
    let owned = OwnedRun::retain(directory);
    let rescue = owned.database.clone();
    let observation = rescue.observe();
    let report = owned
        .execute(|run| {
            Box::pin(async move {
                let music = run.directory.path().join("music");
                std::fs::create_dir(&music)?;
                run.services = Some(hito_core::open(run.config(music)?).await?);
                Err(SafeCode("controlled_startup_failure").into())
            })
        })
        .await;
    let retired_before_return = observation.retirement_started().now_or_never().is_some();
    let rescued = rescue.retire(Duration::from_secs(5)).await;
    drop(rescue);
    assert!(
        rescued.resources_retired && weak.upgrade().is_none(),
        "control resources did not retire"
    );
    assert!(
        retired_before_return && report.sqlite_resources_retired,
        "startup error returned before SQLite resources retired"
    );
    assert_ne!(report.work, "complete");
}

#[tokio::test]
async fn unwinding_work_retires_core_and_returns_a_safe_failure() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let weak = Arc::downgrade(&directory);
    let owned = OwnedRun::retain(directory);
    let rescue = owned.database.clone();
    let observation = rescue.observe();
    let outcome = std::panic::AssertUnwindSafe(owned.execute(|run| {
        Box::pin(async move {
            let music = run.directory.path().join("music");
            std::fs::create_dir(&music)?;
            run.services = Some(hito_core::open(run.config(music)?).await?);
            panic!("owned controlled workflow unwind");
        })
    }))
    .catch_unwind()
    .await;
    let retired_before_return = observation.retirement_started().now_or_never().is_some();
    let rescued = rescue.retire(Duration::from_secs(5)).await;
    drop(rescue);
    assert!(
        rescued.resources_retired && weak.upgrade().is_none(),
        "unwind control resources did not retire"
    );
    assert!(
        retired_before_return,
        "unwind bypassed explicit Core retirement"
    );
    assert!(
        outcome
            .is_ok_and(|report| report.work == "work_panicked" && report.sqlite_resources_retired),
        "unwind escaped the safe workflow result"
    );
}

#[test]
fn previous_success_artifacts_prevent_a_new_run_without_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let environment = directory.path().join("native-environment.json");
    let previous = b"owned previous success evidence";
    std::fs::write(&environment, previous).unwrap();
    let opened = RunOutput::open(directory.path().to_owned());
    let unchanged = std::fs::read(&environment).unwrap() == previous;
    assert!(unchanged, "previous result artifact was overwritten");
    assert!(opened.is_err(), "new run admitted a stale success artifact");
}

#[tokio::test]
async fn rejected_controller_preserves_partial_results_after_all_resources_close() {
    let output_directory = tempfile::tempdir().unwrap();
    let output = RunOutput::open(output_directory.path().to_owned()).unwrap();
    let mut control = PublicControl::open(true).await;
    let replacement = Measurements::open(output_directory.path(), 5).unwrap();
    let previous = std::mem::replace(&mut control.rig.samples, replacement);
    let previous_closed = previous.finish().is_ok();
    let PublicControl {
        directory,
        database,
        services,
        player,
        owner,
        token,
        rig,
    } = control;
    let weak = Arc::downgrade(&directory);
    drop(player);
    let owned = OwnedRun {
        directory,
        database,
        services: Some(services),
        player_owner: Some(owner),
        monitor: None,
        provider: None,
        rig: Some(rig),
        metadata: None,
        retirement_budget: Duration::from_secs(5),
    };
    let report = owned
        .execute(move |run| {
            Box::pin(async move {
                run.services
                    .as_ref()
                    .unwrap()
                    .identity
                    .logout(&token)
                    .await?;
                let rig = run.rig.as_mut().unwrap();
                let observed = rig.settle().await.map(|_| {
                    let now = Instant::now();
                    (now, None, now)
                });
                let fields = crate::measurement::start_fields(
                    "local_start",
                    "FLAC",
                    0,
                    0,
                    "owned",
                    &observed,
                    "",
                );
                rig.samples.record(0, &fields)?;
                observed?;
                Ok(())
            })
        })
        .await;
    let published = output.finish(&report);
    let rows = std::fs::read_to_string(output_directory.path().join("native-warmups.csv")).unwrap();
    let status: serde_json::Value = serde_json::from_slice(
        &std::fs::read(output_directory.path().join("native-run.json")).unwrap(),
    )
    .unwrap();
    assert!(
        previous_closed && weak.upgrade().is_none() && report.sqlite_resources_retired,
        "rejected-controller resources did not close"
    );
    assert!(
        published.is_ok() && report.samples == Some((0, 1)),
        "partial failed attempt was not retained"
    );
    assert!(
        rows.lines().count() == 2 && rows.lines().nth(1).unwrap().split(',').nth(6) == Some("0"),
        "failed attempt row was lost"
    );
    assert!(
        status["state"] == "failed"
            && !output_directory
                .path()
                .join("native-environment.json")
                .exists(),
        "failed run published success artifacts"
    );
}

#[tokio::test]
async fn retirement_deadline_is_reported_while_storage_waits_for_real_leases() {
    use hito_core::test_support::database_owner::RetirementGate;
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let weak = Arc::downgrade(&directory);
    let mut owned = OwnedRun::retain(directory);
    owned.retirement_budget = Duration::from_millis(20);
    let observation = owned.database.observe();
    let gate = Arc::new(RetirementGate::new());
    let held = gate.clone();
    let execute = owned.execute(move |run| {
        Box::pin(async move {
            let music = run.directory.path().join("music");
            std::fs::create_dir(&music)?;
            run.services = Some(hito_core::open(run.config(music)?).await?);
            run.database.hold_retirement(&held);
            Ok(())
        })
    });
    let release = async {
        let entered = tokio::time::timeout(Duration::from_secs(2), gate.entered())
            .await
            .is_ok();
        let late = tokio::time::timeout(Duration::from_secs(2), observation.retirement_deadline())
            .await
            .is_ok();
        let retained = weak.upgrade().is_some() && observation.active_resources() > 0;
        gate.release();
        (entered, late, retained)
    };
    let (report, (entered, late, retained)) = tokio::join!(execute, release);
    assert!(
        entered && late && retained,
        "control did not hold actual worker-local retirement"
    );
    assert!(
        report.sqlite_resources_retired && weak.upgrade().is_none(),
        "released leases retained temporary storage"
    );
    assert!(
        report.sqlite_deadline_exceeded
            && report
                .cleanup_failures
                .contains(&"sqlite_retirement_deadline")
            && !report.successful(),
        "cleanup deadline was hidden by successful work"
    );
}

#[tokio::test]
async fn invalid_core_constructor_retires_the_retained_directory() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let weak = Arc::downgrade(&directory);
    let outside = tempfile::tempdir().unwrap();
    let invalid_path = outside.path().join("invalid.db");
    let report = OwnedRun::retain(directory)
        .execute(move |run| {
            Box::pin(async move {
                let mut config = run.config(run.directory.path().join("music"))?;
                config.database = invalid_path;
                run.services = Some(hito_core::open(config).await?);
                Ok(())
            })
        })
        .await;
    assert!(
        weak.upgrade().is_none() && report.sqlite_resources_retired,
        "invalid constructor retained SQLite storage"
    );
    assert!(
        !report.successful(),
        "invalid constructor was reported as successful"
    );
}

#[tokio::test]
async fn authorization_and_claim_startup_errors_close_the_player_and_retire_core() {
    for invalid_actor in [true, false] {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let weak = Arc::downgrade(&directory);
        let report = OwnedRun::retain(directory)
            .execute(move |run| {
                Box::pin(async move {
                    let music = run.directory.path().join("music");
                    std::fs::create_dir(&music)?;
                    run.services = Some(hito_core::open(run.config(music)?).await?);
                    let services = run.services.as_ref().unwrap();
                    let (player, owner) = Player::start(services.clone());
                    run.player_owner = Some(owner);
                    if invalid_actor {
                        services
                            .identity
                            .authenticate("owned invalid session")
                            .await?;
                    } else {
                        let session = services
                            .identity
                            .setup(Credentials {
                                username: "workflow-admin".into(),
                                password: "owned control password".into(),
                            })
                            .await?;
                        let actor = services.identity.authenticate(&session.token).await?;
                        player.claim(&actor, Uuid::new_v4()).await?;
                    }
                    Err(SafeCode("control_expected_startup_rejection").into())
                })
            })
            .await;
        assert!(
            weak.upgrade().is_none() && report.sqlite_resources_retired,
            "authorization or claim error retained storage"
        );
        assert_ne!(report.work, "complete");
        assert!(
            report.cleanup_failures.is_empty(),
            "startup control did not close cleanly"
        );
    }
}

#[tokio::test]
async fn early_failure_preserves_reader_cleanup_failure_and_retires_core() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let weak = Arc::downgrade(&directory);
    let report = OwnedRun::retain(directory)
        .execute(|run| {
            Box::pin(async move {
                let music = run.directory.path().join("music");
                std::fs::create_dir(&music)?;
                run.services = Some(hito_core::open(run.config(music)?).await?);
                let mut command = Command::new("python3");
                command.args(["-c", "pass"]);
                run.monitor = Some(Monitor::start(command)?);
                let ended = run
                    .monitor
                    .as_mut()
                    .unwrap()
                    .silent_until(Duration::ZERO, Instant::now() + Duration::from_secs(2))
                    .await;
                if ended.is_ok() {
                    return Err(SafeCode("control_expected_reader_eof").into());
                }
                Err(SafeCode("controlled_early_failure").into())
            })
        })
        .await;
    assert!(
        weak.upgrade().is_none() && report.sqlite_resources_retired,
        "partial startup resources did not retire"
    );
    assert_ne!(report.work, "complete");
    assert!(
        report.cleanup_failures.contains(&"monitor_close_failed"),
        "early failure hid the recorder cleanup failure"
    );
}
