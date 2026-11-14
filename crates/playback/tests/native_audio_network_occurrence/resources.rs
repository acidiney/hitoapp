use super::super::fixture;
use futures_util::FutureExt;
use hito_audio::test_support::{
    capture,
    output_witness::{OutputReport, OutputWitness, WitnessError},
};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerOwner};
use hito_testkit::fake_plugin::Gate;
use std::{panic::AssertUnwindSafe, sync::Arc};
use uuid::Uuid;

pub(super) struct Resources {
    pub fixture: Option<fixture::Fixture>,
    pub native: Option<(Player, PlayerOwner)>,
    pub capture: Option<capture::Capture>,
    pub witness: Option<OutputWitness>,
    pub gates: std::sync::Mutex<Vec<Arc<Gate>>>,
}

pub(super) struct Cleanup {
    pub output: Option<Result<OutputReport, WitnessError>>,
    pub silent: bool,
    pub target_audio: f64,
}

impl Resources {
    pub fn new(fixture: fixture::Fixture) -> Self {
        Self {
            fixture: Some(fixture),
            native: None,
            capture: None,
            witness: None,
            gates: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn hold_audio(&self) -> Arc<Gate> {
        let gate = self.fixture.as_ref().unwrap().hold_audio();
        self.gates.lock().unwrap().push(gate.clone());
        gate
    }

    pub async fn finish(mut self, sentinel_progress: bool) -> Cleanup {
        for gate in self.gates.lock().unwrap().iter() {
            let _ = std::panic::catch_unwind(AssertUnwindSafe(|| gate.open()));
        }
        if let Some(fixture) = &self.fixture {
            let _ = std::panic::catch_unwind(AssertUnwindSafe(|| fixture.clear_disconnect()));
        }
        let owner_joined = match self.native.take() {
            Some((player, owner)) => {
                let joined = AssertUnwindSafe(owner.close())
                    .catch_unwind()
                    .await
                    .is_ok_and(|result| result.is_ok());
                drop(player);
                joined
            }
            None => false,
        };
        let output = self.witness.take().and_then(|witness| {
            std::panic::catch_unwind(AssertUnwindSafe(|| witness.finish())).ok()
        });
        let (silent, target_audio) = if let Some(capture) = self.capture.as_ref() {
            std::panic::catch_unwind(AssertUnwindSafe(|| {
                capture.after(capture.mark(), 0.3);
                let recent = capture.recent(0.1);
                (
                    capture::energy(&recent) < 0.000001,
                    capture::amplitude(&recent, 440.),
                )
            }))
            .unwrap_or((false, 1.))
        } else {
            (false, 1.)
        };
        drop(self.capture.take());
        let preserved = if let Some(fixture) = &self.fixture {
            AssertUnwindSafe(async {
                if sentinel_progress {
                    fixture.occurrence.preserved_target_and_files().await
                } else {
                    fixture.occurrence.preserved().await
                }
            })
            .catch_unwind()
            .await
            .is_ok_and(|result| result)
        } else {
            false
        };
        let protocol = self.fixture.as_ref().is_some_and(|fixture| {
            std::panic::catch_unwind(AssertUnwindSafe(|| fixture.occurrence.protocol_clean()))
                .unwrap_or(false)
        });
        let providers_joined = match self.fixture.take() {
            Some(fixture) => AssertUnwindSafe(fixture.close())
                .catch_unwind()
                .await
                .is_ok_and(|result| result),
            None => false,
        };
        assert!(owner_joined && providers_joined && preserved && protocol);
        assert!(output.is_some());
        if let Some(Err(error)) = &output {
            assert!(!matches!(
                error,
                WitnessError::ActiveWriters | WitnessError::Overflow
            ));
        }
        Cleanup {
            output,
            silent,
            target_audio,
        }
    }
}

pub(super) fn command(
    lease: Uuid,
    sequence: u64,
    revision: u64,
    action: PlayerAction,
) -> PlayerCommand {
    PlayerCommand {
        id: Uuid::new_v4(),
        lease,
        sequence,
        revision,
        action,
    }
}
