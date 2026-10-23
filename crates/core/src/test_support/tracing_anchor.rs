use std::sync::OnceLock;

static ANCHOR: OnceLock<tracing::Dispatch> = OnceLock::new();

pub fn anchor_callsite_interest() {
    ANCHOR.get_or_init(|| tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()));
}
