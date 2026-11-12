use super::resources::SafeCode;
use super::{Failure, capture::Monitor, measurement::Measurements, provider::ProviderProcess};
use hito_core::Actor;
use hito_playback::{
    Player, PlayerAction, PlayerCommand, PlayerEntryRef, PlayerPhase, PlayerSnapshot,
};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use uuid::Uuid;

pub(super) struct Rig {
    player: Player,
    actor: Actor,
    lease: Uuid,
    sequence: u64,
    revision: u64,
    retired: bool,
    state: watch::Receiver<PlayerSnapshot>,
    pub(super) monitor: Monitor,
    pub(super) samples: Measurements,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum ControlFailure {
    #[error("player_command_deadline")]
    Deadline,
    #[error("player_command_rejected")]
    Rejected,
    #[error("controller_retired")]
    Retired,
    #[error("start_observation_deadline")]
    ObservationDeadline,
}

pub(super) type Started = (Instant, Option<Instant>, Instant);

impl Rig {
    pub(super) fn new(
        player: Player,
        actor: Actor,
        lease: Uuid,
        state: watch::Receiver<PlayerSnapshot>,
        monitor: Monitor,
        samples: Measurements,
    ) -> Self {
        Self {
            player,
            actor,
            lease,
            sequence: 0,
            revision: 0,
            retired: false,
            state,
            monitor,
            samples,
        }
    }

    async fn submit_until(
        &mut self,
        action: PlayerAction,
        deadline: Instant,
    ) -> Result<(), Failure> {
        if self.retired {
            return Err(ControlFailure::Retired.into());
        }
        if Instant::now() >= deadline {
            self.retired = true;
            return Err(ControlFailure::Deadline.into());
        }
        let Some(sequence) = self.sequence.checked_add(1) else {
            self.retired = true;
            return Err(ControlFailure::Rejected.into());
        };
        let command = PlayerCommand {
            id: Uuid::new_v4(),
            lease: self.lease,
            sequence,
            revision: self.revision,
            action,
        };
        let result = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.player.submit(&self.actor, command),
        )
        .await;
        let receipt = match result {
            Ok(Ok(receipt)) => receipt,
            Ok(Err(_)) => {
                self.retired = true;
                return Err(ControlFailure::Rejected.into());
            }
            Err(_) => {
                self.retired = true;
                return Err(ControlFailure::Deadline.into());
            }
        };
        if Instant::now() >= deadline {
            self.retired = true;
            return Err(ControlFailure::Deadline.into());
        }
        self.sequence = sequence;
        self.revision = receipt.revision;
        Ok(())
    }

    pub(super) fn finish(self) -> (Result<(), Failure>, Result<(usize, usize), Failure>) {
        let Self {
            player,
            state,
            monitor,
            samples,
            ..
        } = self;
        drop(player);
        drop(state);
        monitor.disarm();
        (monitor.finish(), samples.finish())
    }

    pub(super) async fn settle(&mut self) -> Result<(), Failure> {
        self.settle_until(Instant::now() + Duration::from_secs(10))
            .await
    }

    async fn settle_until(&mut self, deadline: Instant) -> Result<(), Failure> {
        self.submit_until(PlayerAction::Pause {}, deadline).await?;
        self.monitor
            .silent_until(Duration::from_millis(150), deadline)
            .await?;
        Ok(())
    }

    pub(super) async fn start_once(&mut self, recording: &str) -> Result<Started, Failure> {
        self.monitor.silent_for(Duration::from_millis(150)).await?;
        self.monitor.arm();
        let started = Instant::now();
        self.start_until(recording, started, started + Duration::from_secs(15))
            .await
    }

    async fn start_until(
        &mut self,
        recording: &str,
        started: Instant,
        deadline: Instant,
    ) -> Result<Started, Failure> {
        if Instant::now() >= deadline {
            self.retired = true;
            return Err(ControlFailure::Deadline.into());
        }
        self.submit_until(
            PlayerAction::Replace {
                recordings: vec![Uuid::parse_str(recording)?],
                start: 0,
            },
            deadline,
        )
        .await?;
        self.submit_until(PlayerAction::Resume {}, deadline).await?;
        self.observe_start_until(started, deadline).await
    }

    async fn observe_start_until(
        &mut self,
        started: Instant,
        deadline: Instant,
    ) -> Result<Started, Failure> {
        let mut playing = None;
        loop {
            if Instant::now() >= deadline {
                return Err(ControlFailure::ObservationDeadline.into());
            }
            if playing.is_none() && self.state.borrow().playing {
                playing = Some(Instant::now());
            }
            if let Some(audible) = self.monitor.first_audible_after(started)? {
                if Instant::now() >= deadline {
                    return Err(ControlFailure::ObservationDeadline.into());
                }
                return Ok((started, playing, audible));
            }
            tokio::time::sleep(Duration::from_micros(500)).await;
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Recovery<'a> {
    pub(super) recording: &'a str,
    pub(super) name: &'a str,
    pub(super) a: &'a str,
    pub(super) b: &'a str,
    pub(super) refreshed: bool,
    pub(super) latency: u64,
}

pub(super) async fn recovery_once(
    rig: &mut Rig,
    provider: &mut ProviderProcess,
    scenario: &Recovery<'_>,
) -> Result<(Instant, Option<Instant>, Instant), Failure> {
    let Recovery {
        recording,
        name,
        a,
        b,
        refreshed,
        latency,
    } = *scenario;
    rig.start_once(recording).await?;
    let input_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let recording_id = Uuid::parse_str(recording)?;
    let initial = tokio::time::timeout_at(
        input_deadline,
        rig.state.wait_for(|state| {
            state
                .current
                .is_some_and(|entry| entry.recording_id == recording_id)
                && state
                    .source_id
                    .as_ref()
                    .is_some_and(|source| !source.is_empty())
                && state.playing
                && state.phase == PlayerPhase::Ready
        }),
    )
    .await
    .map_err(|_| SafeCode("recovery_input_deadline"))?
    .map_err(|_| SafeCode("player_observation_closed"))?
    .clone();
    let entry = initial.current.ok_or(SafeCode("recovery_input_missing"))?;
    let original_source = initial
        .source_id
        .ok_or(SafeCode("recovery_input_missing"))?;
    let selection = initial.selection;
    let mut position = rig.player.subscribe_position(&rig.actor, rig.lease).await?;
    tokio::time::timeout_at(
        input_deadline,
        position.wait_for(|position| {
            position.position_ms >= 1500
                && position.entry == Some(entry.id)
                && position.selection == selection
                && recovered_state(&rig.state.borrow(), entry, selection, &original_source)
        }),
    )
    .await
    .map_err(|_| SafeCode("recovery_input_deadline"))?
    .map_err(|_| SafeCode("player_observation_closed"))?;
    let mut active = a;
    if refreshed {
        provider.sources(name, &[b])?;
    } else {
        if provider.streams(a)? == 0 {
            active = b;
        }
        provider.unpace(if active == a { b } else { a })?;
    }
    provider.latency(latency)?;
    rig.monitor.arm();
    let dropped = Instant::now();
    let deadline = dropped + Duration::from_secs(15);
    let fault = provider.drop_source_until(active, deadline)?;
    tracing::debug!(
        event = "benchmark_source_drop",
        before = fault.before,
        after = fault.after,
        provider_epoch_ms = fault.at_ms
    );
    if Instant::now() >= deadline {
        return Err(SafeCode("recovery_deadline").into());
    }
    if fault.before == 0 || fault.after != 0 {
        return Err(SafeCode("recovery_fault_not_active").into());
    }
    let deadline_at = tokio::time::Instant::from_std(deadline);
    let replacement = tokio::time::timeout_at(
        deadline_at,
        rig.state.wait_for(|state| {
            state.source_id.as_deref().is_some_and(|source| {
                source != original_source && recovered_state(state, entry, selection, source)
            })
        }),
    )
    .await
    .map_err(|_| SafeCode("recovery_source_deadline"))?
    .map_err(|_| SafeCode("player_observation_closed"))?
    .source_id
    .clone()
    .ok_or(SafeCode("recovery_source_missing"))?;
    let restored = position.borrow().position_ms;
    tokio::time::timeout_at(
        deadline_at,
        position.wait_for(|position| {
            position.position_ms > restored
                && position.entry == Some(entry.id)
                && position.selection == selection
                && recovered_state(&rig.state.borrow(), entry, selection, &replacement)
        }),
    )
    .await
    .map_err(|_| SafeCode("recovery_progress_deadline"))?
    .map_err(|_| SafeCode("player_observation_closed"))?;
    let progress_mark = Instant::now();
    loop {
        if Instant::now() >= deadline {
            return Err(SafeCode("recovery_audio_deadline").into());
        }
        if let Some(audible) = rig.monitor.audible_read_after(progress_mark)? {
            let current = position.borrow();
            if Instant::now() >= deadline {
                return Err(SafeCode("recovery_audio_deadline").into());
            }
            if current.entry != Some(entry.id)
                || current.selection != selection
                || current.position_ms <= restored
                || !recovered_state(&rig.state.borrow(), entry, selection, &replacement)
            {
                return Err(SafeCode("recovery_selection_changed").into());
            }
            return Ok((dropped, rig.monitor.first_silence_after(dropped), audible));
        }
        tokio::time::sleep(Duration::from_micros(500)).await;
    }
}

fn recovered_state(
    state: &PlayerSnapshot,
    entry: PlayerEntryRef,
    selection: u64,
    source: &str,
) -> bool {
    !source.is_empty()
        && state.source_id.as_deref() == Some(source)
        && state.current == Some(entry)
        && state.selection == selection
        && state.phase == PlayerPhase::Ready
        && state.playing
}

#[cfg(test)]
#[path = "workflow/tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "workflow/recovery_tests.rs"]
mod recovery_tests;
