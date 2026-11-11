use super::*;
use hito_core::{
    CoreConfig, Credentials, HostDeviceId, Services, test_support::database_owner::DatabaseOwner,
};
use hito_playback::PlayerOwner;
use std::{process::Command, sync::Arc};

pub(crate) struct PublicControl {
    pub(crate) directory: Arc<tempfile::TempDir>,
    pub(crate) database: DatabaseOwner,
    pub(crate) services: Services,
    pub(crate) player: Player,
    pub(crate) owner: PlayerOwner,
    pub(crate) token: String,
    pub(crate) rig: Rig,
}

impl PublicControl {
    pub(crate) async fn open(native_empty: bool) -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let database = DatabaseOwner::retain(directory.clone());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("workflow-control").unwrap(),
        );
        database.attach(&mut config);
        let services = hito_core::open(config).await.unwrap();
        let session = services
            .identity
            .setup(Credentials {
                username: "workflow-admin".into(),
                password: "owned control password".into(),
            })
            .await
            .unwrap();
        let actor = services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        let (player, owner) = if native_empty {
            Player::native(services.clone())
        } else {
            Player::start(services.clone())
        };
        let attached = player
            .claim(&actor, player.inspect(&actor).await.unwrap().epoch)
            .await
            .unwrap();
        let state = player.subscribe(&actor, attached.lease).await.unwrap();
        let mut command = Command::new("python3");
        command.args(["-c", "import os\nwhile True: os.write(1,bytes(1920))"]);
        let monitor = Monitor::start(command).unwrap();
        let output = directory.path().join("output");
        std::fs::create_dir(&output).unwrap();
        let samples = Measurements::open(&output, 5).unwrap();
        let rig = Rig::new(
            player.clone(),
            actor,
            attached.lease,
            state,
            monitor,
            samples,
        );
        Self {
            directory,
            database,
            services,
            player,
            owner,
            token: session.token,
            rig,
        }
    }

    pub(crate) async fn close(self) -> bool {
        let Self {
            directory,
            database,
            services,
            player,
            owner,
            token: _,
            rig,
        } = self;
        let closed = owner.close().await.is_ok();
        let Rig {
            player: rig_player,
            state,
            monitor,
            samples,
            ..
        } = rig;
        drop(rig_player);
        drop(state);
        drop(player);
        let monitor_closed = monitor.finish().is_ok();
        let samples_closed = samples.finish().is_ok();
        drop(services);
        let retired = database.retire(Duration::from_secs(5)).await;
        let weak = Arc::downgrade(&directory);
        drop(database);
        drop(directory);
        closed
            && monitor_closed
            && samples_closed
            && retired.resources_retired
            && !retired.deadline_exceeded
            && weak.upgrade().is_none()
    }
}

#[tokio::test]
async fn rejected_public_pause_cannot_be_replaced_by_quiet_pcm() {
    let mut control = PublicControl::open(false).await;
    control
        .services
        .identity
        .logout(&control.token)
        .await
        .unwrap();
    let result = control.rig.settle().await;
    let cleanup = control.close().await;
    assert!(cleanup, "public control resources did not close and retire");
    assert!(
        result.is_err(),
        "rejected public Pause was replaced by quiet PCM success"
    );
}

#[tokio::test]
async fn valid_public_pause_and_fresh_quiet_interval_settle_successfully() {
    let mut control = PublicControl::open(true).await;
    let before = control
        .player
        .inspect(&control.rig.actor)
        .await
        .unwrap()
        .state;
    let result = control.rig.settle().await;
    let after = control
        .player
        .inspect(&control.rig.actor)
        .await
        .unwrap()
        .state;
    let empty = |state: &Option<PlayerSnapshot>| {
        state
            .as_ref()
            .is_some_and(|state| state.current.is_none() && state.queue_len == 0)
    };
    let unchanged_empty = empty(&before) && empty(&after);
    let cleanup = control.close().await;
    assert!(cleanup, "valid-pause resources did not close and retire");
    assert!(
        unchanged_empty,
        "native-capable controller gained a current recording or queue entry"
    );
    assert!(
        result.is_ok(),
        "valid public Pause plus fresh quiet interval was rejected"
    );
}

#[tokio::test]
async fn rejected_pause_from_empty_native_capable_owner_cannot_be_replaced_by_quiet_pcm() {
    let mut control = PublicControl::open(true).await;
    let empty_before =
        control.rig.state.borrow().current.is_none() && control.rig.state.borrow().queue_len == 0;
    control
        .services
        .identity
        .logout(&control.token)
        .await
        .unwrap();
    let result = control.rig.settle().await;
    let empty_after =
        control.rig.state.borrow().current.is_none() && control.rig.state.borrow().queue_len == 0;
    let cleanup = control.close().await;
    assert!(
        cleanup,
        "native-capable rejection resources did not close and retire"
    );
    assert!(
        empty_before && empty_after,
        "native-capable rejection gained a current recording or queue entry"
    );
    assert!(
        result.is_err(),
        "rejected native-capable Pause was replaced by quiet PCM success"
    );
}

#[tokio::test]
async fn expired_original_command_budget_cannot_submit_or_contaminate_another_control() {
    let mut control = PublicControl::open(false).await;
    let before = control
        .player
        .inspect(&control.rig.actor)
        .await
        .unwrap()
        .state;
    let revision = before.as_ref().unwrap().revision;
    let expired = control
        .rig
        .submit_until(
            PlayerAction::Clear {},
            Instant::now() - Duration::from_millis(1),
        )
        .await;
    let after = control
        .player
        .inspect(&control.rig.actor)
        .await
        .unwrap()
        .state;
    let first_sequence = control
        .player
        .submit(
            &control.rig.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: control.rig.lease,
                sequence: 1,
                revision,
                action: PlayerAction::Clear {},
            },
        )
        .await;
    let probe_state = control
        .player
        .inspect(&control.rig.actor)
        .await
        .unwrap()
        .state;
    let later = control
        .rig
        .submit_until(
            PlayerAction::Clear {},
            Instant::now() + Duration::from_secs(1),
        )
        .await;
    let final_state = control
        .player
        .inspect(&control.rig.actor)
        .await
        .unwrap()
        .state;
    let static_deadline = expired
        .as_ref()
        .err()
        .is_some_and(|error| error.to_string() == "player_command_deadline");
    let unchanged = before == after;
    let no_later_mutation = probe_state == final_state;
    let sequence_available = first_sequence.is_ok();
    let cleanup = control.close().await;
    eprintln!(
        "{{\"event\":\"expired_public_command_control\",\"expired_rejected\":{},\"state_unchanged\":{unchanged},\"sequence_available\":{sequence_available},\"later_rejected\":{}}}",
        expired.is_err(),
        later.is_err()
    );
    assert!(
        cleanup,
        "expired-command resources did not close and retire"
    );
    assert!(
        static_deadline,
        "expired original command budget did not return static deadline failure"
    );
    assert!(
        unchanged && sequence_available,
        "expired command changed public state or consumed a sequence"
    );
    assert!(
        later.is_err() && no_later_mutation,
        "failed controller was reused for another trial"
    );
}

#[tokio::test]
async fn settle_pause_and_quiet_observation_share_the_original_budget() {
    let mut control = PublicControl::open(true).await;
    let mut command = Command::new("python3");
    command.args(["-c", "import sys; sys.stdin.read()"]);
    let held = Monitor::start(command).unwrap();
    let previous = std::mem::replace(&mut control.rig.monitor, held);
    let previous_closed = previous.finish().is_ok();
    let deadline = Instant::now() + Duration::from_millis(30);
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        control.rig.settle_until(deadline),
    )
    .await;
    let empty =
        control.rig.state.borrow().current.is_none() && control.rig.state.borrow().queue_len == 0;
    let returned_deadline = result
        .as_ref()
        .ok()
        .and_then(|result| result.as_ref().err())
        .is_some_and(|error| {
            ["monitor_quiet_deadline", "player_command_deadline"]
                .contains(&error.to_string().as_str())
        });
    let cleanup = control.close().await;
    assert!(
        previous_closed && cleanup,
        "shared-settle-budget resources did not close and retire"
    );
    assert!(empty, "settle control gained a queue entry or recording");
    assert!(
        returned_deadline,
        "settle observation received a fresh budget after Pause"
    );
}

#[tokio::test]
async fn original_start_budget_is_checked_before_replace() {
    let mut control = PublicControl::open(false).await;
    let before = control
        .player
        .inspect(&control.rig.actor)
        .await
        .unwrap()
        .state;
    let expired = Instant::now() - Duration::from_millis(1);
    let result = control
        .rig
        .start_until(&Uuid::new_v4().to_string(), expired, expired)
        .await;
    let after = control
        .player
        .inspect(&control.rig.actor)
        .await
        .unwrap()
        .state;
    let deadline = result
        .as_ref()
        .err()
        .is_some_and(|error| error.to_string() == "player_command_deadline");
    let cleanup = control.close().await;
    assert!(cleanup, "expired-start resources did not close and retire");
    assert!(
        deadline && before == after,
        "original start budget was not checked before Replace"
    );
}

#[tokio::test]
async fn original_start_observation_budget_is_not_restarted() {
    let mut control = PublicControl::open(false).await;
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        control
            .rig
            .observe_start_until(started, started + Duration::from_millis(30)),
    )
    .await;
    let deadline = result
        .as_ref()
        .ok()
        .and_then(|result| result.as_ref().err())
        .is_some_and(|error| error.to_string() == "start_observation_deadline");
    let cleanup = control.close().await;
    assert!(
        cleanup,
        "start-observation resources did not close and retire"
    );
    assert!(
        deadline,
        "start observation replaced the original budget with a fresh window"
    );
}
