use hito_plugin_sdk::FeaturedItem;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const ANSWER_LIFETIME: Duration = Duration::from_secs(600);
const FAILURE_LIFETIME: Duration = Duration::from_secs(30);
const MAX_ENTRIES: usize = 64;

const ARTWORK_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Default)]
pub(crate) struct FeaturedCache {
    entries: Arc<Mutex<HashMap<(String, i64), Entry>>>,
    covers: Arc<Mutex<Covers>>,
}

#[derive(Default)]
struct Covers {
    held: HashMap<(String, i64, String), Cover>,
    bytes: usize,
    clock: u64,
}

struct Cover {
    content_type: &'static str,
    bytes: Arc<Vec<u8>>,
    used: u64,
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

impl FeaturedCache {
    pub(super) fn cover(
        &self,
        installation: &str,
        revision: i64,
        artwork: &str,
    ) -> Option<(&'static str, Vec<u8>)> {
        let mut covers = self.covers.lock().ok()?;
        covers.clock += 1;
        let clock = covers.clock;
        let cover =
            covers
                .held
                .get_mut(&(installation.to_owned(), revision, artwork.to_owned()))?;
        cover.used = clock;
        Some((cover.content_type, cover.bytes.as_ref().clone()))
    }

    pub(super) fn keep_cover(
        &self,
        installation: &str,
        revision: i64,
        artwork: &str,
        content_type: &'static str,
        bytes: &[u8],
    ) {
        if bytes.len() > ARTWORK_BYTES {
            return;
        }
        let Ok(mut covers) = self.covers.lock() else {
            return;
        };
        let key = (installation.to_owned(), revision, artwork.to_owned());
        if let Some(old) = covers.held.remove(&key) {
            covers.bytes -= old.bytes.len();
        }
        while covers.bytes + bytes.len() > ARTWORK_BYTES {
            let Some(oldest) = covers
                .held
                .iter()
                .min_by_key(|(_, cover)| cover.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(old) = covers.held.remove(&oldest) {
                covers.bytes -= old.bytes.len();
            }
        }
        covers.clock += 1;
        let used = covers.clock;
        covers.bytes += bytes.len();
        covers.held.insert(
            key,
            Cover {
                content_type,
                bytes: Arc::new(bytes.to_vec()),
                used,
            },
        );
    }
}
