use super::{Failure, capture::Monitor, measurement::Measurements, provider::ProviderProcess};
use hito_core::Actor;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerSnapshot};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use uuid::Uuid;

pub(super) struct Rig {
    player: Player,
    actor: Actor,
    lease: Uuid,
    sequence: u64,
    revision: u64,
    state: watch::Receiver<PlayerSnapshot>,
    pub(super) monitor: Monitor,
    pub(super) samples: Measurements,
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
            state,
            monitor,
            samples,
        }
    }

    async fn submit(&mut self, action: PlayerAction) -> Result<(), Failure> {
        self.sequence += 1;
        let receipt = self
            .player
            .submit(
                &self.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: self.lease,
                    sequence: self.sequence,
                    revision: self.revision,
                    action,
                },
            )
            .await
            .map_err(|error| format!("{error:?}"))?;
        self.revision = receipt.revision;
        Ok(())
    }

    pub(super) async fn settle(&mut self) -> Result<(), Failure> {
        let _ = self.submit(PlayerAction::Pause {}).await;
        self.monitor.silent_for(Duration::from_millis(150)).await?;
        Ok(())
    }

    pub(super) async fn start_once(&mut self, recording: &str) -> Result<Started, Failure> {
        self.monitor.silent_for(Duration::from_millis(150)).await?;
        self.monitor.arm();
        let started = Instant::now();
        self.submit(PlayerAction::Replace {
            recordings: vec![Uuid::parse_str(recording)?],
            start: 0,
        })
        .await?;
        self.submit(PlayerAction::Resume {}).await?;
        let mut playing = None;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if playing.is_none() && self.state.borrow().playing {
                playing = Some(Instant::now());
            }
            if let Some(audible) = self.monitor.first_audible_after(started)? {
                return Ok((started, playing, audible));
            }
            if Instant::now() > deadline {
                return Err("no audible output within 15 seconds".into());
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
    let mut position = rig.player.subscribe_position(&rig.actor, rig.lease).await?;
    tokio::time::timeout(
        Duration::from_secs(10),
        position.wait_for(|position| position.position_ms >= 1500),
    )
    .await??;
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
    provider.drop_source(active)?;
    let wait = Instant::now() + Duration::from_secs(15);
    let mut silence = None;
    loop {
        silence = silence.or_else(|| rig.monitor.first_silence_after(dropped));
        if let Some(gap) = silence
            && let Some(audible) = rig.monitor.first_audible_after(gap)?
        {
            return Ok((dropped, silence, audible));
        }
        if Instant::now() > wait {
            return Err("no recovered audio within 15 seconds".into());
        }
        tokio::time::sleep(Duration::from_micros(500)).await;
    }
}
