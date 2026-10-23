use std::sync::OnceLock;

static ANCHOR: OnceLock<tracing::Dispatch> = OnceLock::new();

/// Keeps a second dispatcher registered for the life of the test process so that tracing
/// recomputes callsite interest across every scoped capture instead of consulting only the
/// registering thread's default, which hides events from captures owned by other threads.
pub fn ensure() {
    ANCHOR.get_or_init(|| tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()));
}
