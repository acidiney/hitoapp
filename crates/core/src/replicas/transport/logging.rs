use std::time::Instant;

pub(super) struct Exchange {
    started: Instant,
    dispatcher: tracing::Dispatch,
    span: tracing::Span,
    outcome: &'static str,
}

impl Exchange {
    pub(super) fn started() -> Self {
        Self {
            started: Instant::now(),
            dispatcher: tracing::dispatcher::get_default(Clone::clone),
            span: tracing::Span::current(),
            outcome: "cancelled",
        }
    }

    pub(super) fn finished(&mut self, outcome: &'static str) {
        self.outcome = outcome;
    }
}

impl Drop for Exchange {
    fn drop(&mut self) {
        tracing::dispatcher::with_default(&self.dispatcher, || {
            let _entered = self.span.enter();
            tracing::debug!(
                event = "replica_exchange_finished",
                outcome = self.outcome,
                elapsed_ms = self.started.elapsed().as_millis() as u64,
                "Replica exchange finished"
            );
        });
    }
}
