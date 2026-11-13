use super::{
    PluginHealth, PluginHealthState, PluginInstallation, Plugins, transport::health::is_unreachable,
};
use crate::{Actor, Error};
use futures_util::{StreamExt, stream};
use hito_plugin_sdk::HealthState;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const FRESH_FOR: Duration = Duration::from_secs(30);
const READ_BUDGET: Duration = Duration::from_secs(4);
const CONCURRENT_READS: usize = 8;
const MAX_CACHED: usize = 128;

type Key = (String, i64);
type Answer = (Instant, Option<PluginHealth>);

#[derive(Clone, Eq, Hash, PartialEq)]
pub(super) struct ConnectionGeneration {
    pub account: String,
    pub installation: String,
    pub installation_revision: i64,
    pub service: String,
    pub service_revision: i64,
}

#[derive(Clone, Copy)]
struct ProtectedObservation {
    sequence: u64,
    observed: Instant,
}

pub(crate) struct CredentialTicket {
    identity: Arc<()>,
    sequence: u64,
}

struct CredentialObservation {
    identity: Arc<()>,
    last_dispatch: Instant,
    last_sequence: u64,
    rejected: Option<ProtectedObservation>,
    verified: Option<ProtectedObservation>,
}

impl Default for CredentialObservation {
    fn default() -> Self {
        Self {
            identity: Arc::new(()),
            last_dispatch: Instant::now(),
            last_sequence: 0,
            rejected: None,
            verified: None,
        }
    }
}

impl CredentialObservation {
    fn prune(&mut self) {
        if self
            .rejected
            .is_some_and(|rejection| rejection.observed.elapsed() >= FRESH_FOR)
        {
            self.rejected = None;
        }
        if self
            .verified
            .is_some_and(|success| success.observed.elapsed() >= FRESH_FOR)
        {
            self.verified = None;
        }
    }

    fn last_observed(&self) -> Instant {
        self.last_dispatch
            .max(
                self.rejected
                    .map_or(self.last_dispatch, |rejection| rejection.observed),
            )
            .max(
                self.verified
                    .map_or(self.last_dispatch, |success| success.observed),
            )
    }
}

type CredentialObservations = HashMap<ConnectionGeneration, CredentialObservation>;

fn credential_entry(
    observations: &mut CredentialObservations,
    generation: ConnectionGeneration,
) -> &mut CredentialObservation {
    observations.retain(|_, observation| {
        observation.prune();
        observation.last_observed().elapsed() < FRESH_FOR
    });
    if observations.len() >= MAX_CACHED
        && !observations.contains_key(&generation)
        && let Some(oldest) = observations
            .iter()
            .min_by_key(|(_, observation)| observation.last_observed())
            .map(|(key, _)| key.clone())
    {
        observations.remove(&oldest);
    }
    observations.entry(generation).or_default()
}

fn indexed_entry<'a>(
    observations: &'a mut CredentialObservations,
    generation: &ConnectionGeneration,
    ticket: &CredentialTicket,
) -> Option<&'a mut CredentialObservation> {
    let observation = observations.get_mut(generation)?;
    if !Arc::ptr_eq(&observation.identity, &ticket.identity) {
        return None;
    }
    observation.prune();
    (observation.last_observed().elapsed() < FRESH_FOR).then_some(observation)
}

#[derive(Clone, Default)]
pub(super) struct Cache {
    entries: Arc<Mutex<HashMap<Key, Answer>>>,
    credentials: Arc<Mutex<CredentialObservations>>,
}

impl Cache {
    pub(super) fn credential_ticket(&self, generation: ConnectionGeneration) -> CredentialTicket {
        let started = Instant::now();
        #[cfg(feature = "test-support")]
        tracing::debug!(
            target: "hito_core::credential_ticket_test",
            point = "credential_ticket_before_lock"
        );
        let mut observations = self
            .credentials
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let observation = credential_entry(&mut observations, generation);
        let sequence = match observation.last_sequence.checked_add(1) {
            Some(sequence) => sequence,
            None => {
                *observation = CredentialObservation::default();
                1
            }
        };
        observation.last_sequence = sequence;
        observation.last_dispatch = observation.last_dispatch.max(started);
        CredentialTicket {
            identity: observation.identity.clone(),
            sequence,
        }
    }

    pub(super) fn record_credentials(
        &self,
        generation: ConnectionGeneration,
        ticket: &CredentialTicket,
    ) {
        let mut observations = self
            .credentials
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(observation) = indexed_entry(&mut observations, &generation, ticket) else {
            tracing::debug!(
                event = "plugin_credentials_rejection_ignored",
                outcome = "evicted_or_expired",
                "The request's credential observation is no longer current"
            );
            return;
        };
        if observation
            .verified
            .is_some_and(|success| ticket.sequence < success.sequence)
            || observation
                .rejected
                .is_some_and(|rejection| ticket.sequence <= rejection.sequence)
        {
            tracing::debug!(
                event = "plugin_credentials_rejection_ignored",
                outcome = "older_request",
                "A newer protected request already determined the credential outcome"
            );
            return;
        }
        observation.rejected = Some(ProtectedObservation {
            sequence: ticket.sequence,
            observed: Instant::now(),
        });
        tracing::debug!(
            event = "plugin_credentials_rejected",
            outcome = "credentials_required",
            "Plugin service credentials were rejected"
        );
    }

    fn credentials_required(&self, generation: &ConnectionGeneration) -> bool {
        self.credentials
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(generation)
            .and_then(|observation| observation.rejected)
            .is_some_and(|rejection| rejection.observed.elapsed() < FRESH_FOR)
    }

    pub(super) fn clear_credentials(
        &self,
        generation: &ConnectionGeneration,
        ticket: &CredentialTicket,
    ) {
        let mut observations = self
            .credentials
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(observation) = indexed_entry(&mut observations, generation, ticket) else {
            tracing::debug!(
                event = "plugin_credentials_verification_ignored",
                outcome = "evicted_or_expired",
                "The request's credential observation is no longer current"
            );
            return;
        };
        let sequence = ticket.sequence;
        let cleared = observation
            .rejected
            .is_some_and(|rejection| rejection.sequence < sequence);
        if cleared {
            observation.rejected = None;
        }
        if observation
            .verified
            .is_none_or(|success| success.sequence < sequence)
        {
            observation.verified = Some(ProtectedObservation {
                sequence,
                observed: Instant::now(),
            });
        }
        tracing::debug!(
            event = "plugin_credentials_verified",
            outcome = if cleared { "cleared" } else { "observed" },
            "A checked protected request verified plugin credentials"
        );
    }

    fn fresh(&self, key: &Key) -> Option<Option<PluginHealth>> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        entries
            .get(key)
            .filter(|(read_at, _)| read_at.elapsed() < FRESH_FOR)
            .map(|(_, health)| *health)
    }

    fn remember(&self, key: Key, health: Option<PluginHealth>) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if entries.len() >= MAX_CACHED {
            entries.retain(|_, (read_at, _)| read_at.elapsed() < FRESH_FOR);
        }
        if entries.len() >= MAX_CACHED {
            entries.clear();
        }
        entries.insert(key, (Instant::now(), health));
    }
}

impl Plugins {
    #[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.id()))]
    pub(super) async fn attach_health(
        &self,
        actor: &Actor,
        installations: &mut [PluginInstallation],
    ) {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + READ_BUDGET;
        let mut pending = Vec::new();
        let mut answers = Vec::new();
        for (index, installation) in installations.iter_mut().enumerate() {
            let key = (installation.id.clone(), installation.revision);
            match self.health.fresh(&key) {
                Some(health) => {
                    answers.push((index, key, health, false));
                }
                None => pending.push((index, key)),
            }
        }
        let reads = pending.len();
        let received: Vec<_> = stream::iter(pending)
            .map(|(index, key)| async move {
                let read = self.read_health(actor, &key.0, key.1);
                let health = tokio::time::timeout_at(deadline, read)
                    .await
                    .unwrap_or_else(|_| Some(unreachable()));
                (index, key, health, true)
            })
            .buffer_unordered(CONCURRENT_READS)
            .collect()
            .await;
        answers.extend(received);
        let _ = tokio::time::timeout_at(deadline, async {
            for (index, key, health, remember) in answers {
                if tokio::time::Instant::now() >= deadline {
                    break;
                }
                let Ok(operation) = self.operation(actor, &key.0, key.1, &[]).await else {
                    continue;
                };
                if operation.check_current().await.is_err() {
                    continue;
                }
                if tokio::time::Instant::now() >= deadline {
                    break;
                }
                if remember {
                    self.health.remember(key, health);
                }
                installations[index].health = if self
                    .health
                    .credentials_required(&operation.connection_generation())
                {
                    Some(credentials_required())
                } else {
                    health
                };
            }
        })
        .await;
        tracing::debug!(
            reads,
            installation_count = installations.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Plugin health read"
        );
    }

    async fn read_health(&self, actor: &Actor, id: &str, revision: i64) -> Option<PluginHealth> {
        let operation = self.operation(actor, id, revision, &[]).await.ok()?;
        match operation.health().await {
            Ok(Some(report)) => Some(PluginHealth {
                state: match report.state {
                    HealthState::Ok => PluginHealthState::Ok,
                    HealthState::Degraded => PluginHealthState::Degraded,
                },
                reason: report.reason,
                since: report.since,
            }),
            Ok(None) => None,
            Err(Error::PluginCredentialsRequired(_)) => None,
            Err(error) if is_unreachable(&error) => Some(unreachable()),
            Err(_) => None,
        }
    }
}

fn credentials_required() -> PluginHealth {
    PluginHealth {
        state: PluginHealthState::CredentialsRequired,
        reason: None,
        since: None,
    }
}

fn unreachable() -> PluginHealth {
    PluginHealth {
        state: PluginHealthState::Unreachable,
        reason: None,
        since: None,
    }
}
