use crate::library_storage::{root::Root, walking::supported};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const MAX_DEPTH: usize = 64;
pub(super) const MAX_FILES: u64 = 500_000;

pub(super) struct Visit<'a> {
    pub relative: &'a Path,
    pub size: u64,
    pub modified_secs: Option<i64>,
}

/// Visits supported audio files inside the granted root without following
/// links. Returns true when the file cap stopped the walk early.
pub(super) fn walk(root: &Root, mut visit: impl FnMut(Visit<'_>)) -> bool {
    let Ok(entries) = root.directory.entries() else {
        return false;
    };
    let mut files = 0u64;
    let mut stack = vec![(PathBuf::new(), entries)];
    while let Some((parent, entries)) = stack.last_mut() {
        let Some(entry) = entries.next() else {
            stack.pop();
            continue;
        };
        #[cfg(test)]
        if let Some(hook) = WALK_HOOK.lock().unwrap().clone() {
            hook();
        }
        let Ok(entry) = entry else { continue };
        let relative = parent.join(entry.file_name());
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if stack.len() < MAX_DEPTH
                && let Ok(children) = root.directory.read_dir(&relative)
            {
                stack.push((relative, children));
            }
        } else if kind.is_file()
            && supported(&relative)
            && let Ok(metadata) = entry.metadata()
        {
            if files >= MAX_FILES {
                return true;
            }
            files += 1;
            let modified_secs = metadata
                .modified()
                .ok()
                .and_then(|time| time.into_std().duration_since(UNIX_EPOCH).ok())
                .map(|age| age.as_secs() as i64);
            visit(Visit {
                relative: &relative,
                size: metadata.len(),
                modified_secs,
            });
        }
    }
    false
}

pub(super) struct Fingerprint {
    pub value: u64,
    pub capped: bool,
}

pub(super) fn fingerprint(root: &Root) -> Fingerprint {
    let mut total = 0u64;
    let capped = walk(root, |file| {
        let mut hasher = DefaultHasher::new();
        (file.relative, file.size, file.modified_secs).hash(&mut hasher);
        total = total.wrapping_add(hasher.finish());
    });
    Fingerprint {
        value: total,
        capped,
    }
}

#[cfg(test)]
pub(super) static WALK_HOOK: std::sync::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>> =
    std::sync::Mutex::new(None);
