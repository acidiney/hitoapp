use std::time::Instant;

pub(crate) struct WorkflowLog {
    pub started: Instant,
    pub dispatch: tracing::Dispatch,
    pub span: tracing::Span,
    pub event: &'static str,
    pub outcome: &'static str,
    pub count: u64,
    pub mint_dispatched: bool,
}

impl Drop for WorkflowLog {
    fn drop(&mut self) {
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
        let outcome = if self.outcome == "cancelled" && self.mint_dispatched {
            "link_unknown"
        } else {
            self.outcome
        };
        tracing::debug!(
            event = self.event,
            outcome,
            count = self.count,
            elapsed_ms = self.started.elapsed().as_millis() as u64
        );
    }
}
