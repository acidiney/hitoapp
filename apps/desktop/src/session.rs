use hito_desktop::{RecentServers, ServerSession};
use std::{path::PathBuf, sync::Mutex};
use url::Url;

const SERVICE: &str = "org.hito.audio";

pub struct DesktopSession {
    origin: Url,
    data: PathBuf,
    cached: Mutex<Option<Option<String>>>,
}

impl DesktopSession {
    pub fn new(origin: Url, data: PathBuf) -> Self {
        Self {
            origin,
            data,
            cached: Mutex::new(None),
        }
    }

    fn entry(&self) -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, &self.origin.origin().ascii_serialization()).ok()
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Option<Option<String>>> {
        self.cached
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl ServerSession for DesktopSession {
    fn token(&self) -> Option<String> {
        let mut cached = self.cache();
        if let Some(token) = cached.as_ref() {
            return token.clone();
        }
        let token = self.entry().and_then(|entry| entry.get_password().ok());
        *cached = Some(token.clone());
        token
    }

    fn keep(&self, token: &str, username: &str) -> bool {
        let Some(entry) = self.entry() else {
            return false;
        };
        if entry.set_password(token).is_err() {
            return false;
        }
        *self.cache() = Some(Some(token.to_owned()));
        let mut recent = RecentServers::load(&self.data);
        recent.signed_in(&self.origin, username);
        if recent.save(&self.data).is_err() {
            tracing::warn!(
                event = "desktop_recent_save_failed",
                "The list of recent servers could not be saved"
            );
        }
        true
    }

    fn end(&self) {
        if let Some(entry) = self.entry() {
            let _ = entry.delete_credential();
        }
        *self.cache() = Some(None);
    }
}
