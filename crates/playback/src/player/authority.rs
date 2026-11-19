use super::{PlayerError, Result, ownership::State};
use crate::seam::Listener;
use crate::seam::{Authorization, CatalogReads, Seam};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::time::{Interval, MissedTickBehavior};
use uuid::Uuid;

pub(super) struct Report {
    pub lease: Uuid,
    pub recovering: bool,
    pub result: Result<()>,
}
#[derive(Clone)]
struct Demand {
    actor: Listener,
    lease: Uuid,
    recovering: bool,
    recording: Option<Uuid>,
}
pub(super) struct Monitor {
    authorization: Arc<dyn Authorization>,
    catalog: Arc<dyn CatalogReads>,
    demand: Option<Demand>,
    checking: Option<Pin<Box<dyn Future<Output = Report> + Send>>>,
    interval: Interval,
}
impl Monitor {
    pub fn new(seam: &Seam) -> Self {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Self {
            authorization: seam.authorization.clone(),
            catalog: seam.catalog.clone(),
            demand: None,
            checking: None,
            interval,
        }
    }
    pub fn sync(&mut self, state: &State) {
        self.demand = state.owner.as_ref().map(|owned| Demand {
            actor: owned.actor.clone(),
            lease: owned.lease,
            recovering: !owned.authority_available,
            recording: owned
                .queue
                .snapshot()
                .current
                .map(|entry| entry.recording_id),
        });
    }
    pub async fn next(&mut self) -> Report {
        loop {
            tokio::select! {
                biased;
                report = async { self.checking.as_mut().unwrap().await }, if self.checking.is_some() => {
                    self.checking = None;
                    return report;
                },
                _ = self.interval.tick(), if self.checking.is_none() => {
                    let Some(demand) = self.demand.clone() else { continue; };
                    let authorization = self.authorization.clone();
                    let catalog = self.catalog.clone();
                    self.checking = Some(Box::pin(async move {
                        let result = async {
                            authorization.check_session(&demand.actor).await.map_err(PlayerError::from)?;
                            if demand.recovering && let Some(recording) = demand.recording {
                                match catalog.recordings(&demand.actor, &[recording]).await.map_err(PlayerError::from) {
                                    Ok(_) | Err(PlayerError::NotFound) => (),
                                    Err(error) => return Err(error),
                                }
                            }
                            Ok(())
                        }.await;
                        Report { lease: demand.lease, recovering: demand.recovering, result }
                    }));
                },
            }
        }
    }
}

impl State {
    pub fn checked_session(&mut self, lease: Uuid, recovering: bool, result: Result<()>) {
        let Some(owned) = self.owner.as_mut().filter(|owned| owned.lease == lease) else {
            return;
        };
        match result {
            Err(PlayerError::Unauthorized) => {
                self.owner = None;
                self.epoch = Uuid::new_v4();
                tracing::info!(
                    event = "player_session_withdrawn",
                    "Playback ownership cleared"
                );
            }
            Ok(()) if !owned.authority_available && !recovering => (),
            result => owned.authority(result.is_ok(), self.native),
        }
    }
    pub fn failed_request(&mut self, actor: &Listener, error: PlayerError) {
        if matches!(error, PlayerError::Unauthorized | PlayerError::Unavailable)
            && let Some(owned) = self
                .owner
                .as_ref()
                .filter(|owned| owned.actor.same_session(actor))
        {
            self.checked_session(owned.lease, false, Err(error));
        }
    }
}
