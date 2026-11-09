use super::{
    PluginHealth, PluginHealthState, PluginInstallation, Plugins, transport::health::is_unreachable,
};
use crate::Actor;
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

#[derive(Clone, Default)]
pub(super) struct Cache {
    entries: Arc<Mutex<HashMap<Key, Answer>>>,
}

impl Cache {
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
        for (index, installation) in installations.iter_mut().enumerate() {
            let key = (installation.id.clone(), installation.revision);
            match self.health.fresh(&key) {
                Some(health) => installation.health = health,
                None => pending.push((index, key)),
            }
        }
        let reads = pending.len();
        let answers: Vec<_> = stream::iter(pending)
            .map(|(index, key)| async move {
                let read = self.read_health(actor, &key.0, key.1);
                let health = tokio::time::timeout_at(deadline, read)
                    .await
                    .unwrap_or_else(|_| Some(unreachable()));
                (index, key, health)
            })
            .buffer_unordered(CONCURRENT_READS)
            .collect()
            .await;
        for (index, key, health) in answers {
            self.health.remember(key, health);
            installations[index].health = health;
        }
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
            Err(error) if is_unreachable(&error) => Some(unreachable()),
            Err(_) => None,
        }
    }
}

fn unreachable() -> PluginHealth {
    PluginHealth {
        state: PluginHealthState::Unreachable,
        reason: None,
        since: None,
    }
}
