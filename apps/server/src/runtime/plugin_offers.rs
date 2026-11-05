use hito_core::{ConfiguredPluginService, Plugins};
use std::time::Duration;
use tokio::sync::watch;

const FIRST_RETRY: Duration = Duration::from_secs(1);
const LONGEST_RETRY: Duration = Duration::from_secs(300);

pub async fn offer_configured_plugin_services(
    plugins: Plugins,
    services: Vec<ConfiguredPluginService>,
    mut stopped: watch::Receiver<bool>,
) {
    let mut pending = services;
    let mut delay = FIRST_RETRY;
    while !pending.is_empty() {
        let mut unresolved = Vec::new();
        for service in pending {
            if *stopped.borrow() {
                return;
            }
            if plugins.offer_configured(&service).await.is_err() {
                unresolved.push(service);
            }
        }
        pending = unresolved;
        if pending.is_empty() {
            return;
        }
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = stopped.changed() => return,
        }
        delay = (delay * 2).min(LONGEST_RETRY);
    }
}
