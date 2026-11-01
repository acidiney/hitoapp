use hito_plugin_sdk::FeaturedItem;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const ANSWER_LIFETIME: Duration = Duration::from_secs(600);
const FAILURE_LIFETIME: Duration = Duration::from_secs(30);
const MAX_ENTRIES: usize = 64;

#[derive(Clone, Default)]
pub(crate) struct FeaturedCache {
    entries: Arc<Mutex<HashMap<(String, i64), Entry>>>,
}

struct Entry {
    stored: Instant,
    answer: Option<Vec<FeaturedItem>>,
}

impl FeaturedCache {
    pub(super) fn get(
        &self,
        installation: &str,
        revision: i64,
    ) -> Option<Option<Vec<FeaturedItem>>> {
        let mut entries = self.entries.lock().ok()?;
        let key = (installation.to_owned(), revision);
        let entry = entries.get(&key)?;
        let lifetime = if entry.answer.is_some() {
            ANSWER_LIFETIME
        } else {
            FAILURE_LIFETIME
        };
        if entry.stored.elapsed() < lifetime {
            return Some(entry.answer.clone());
        }
        entries.remove(&key);
        None
    }

    pub(super) fn put(&self, installation: &str, revision: i64, answer: Option<Vec<FeaturedItem>>) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        if entries.len() >= MAX_ENTRIES
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.stored)
                .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(
            (installation.to_owned(), revision),
            Entry {
                stored: Instant::now(),
                answer,
            },
        );
    }
}
