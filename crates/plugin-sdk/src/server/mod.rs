mod auth;
mod logging;
mod observe;
mod router;

pub use auth::{
    AuthFailure, PROTOCOL_HEADER, ServiceAuth, bearer_digest, credential_digest, require_protocol,
};
pub use logging::{LoggingInitError, init_logging};
pub use observe::{Finished, Rejection, RequestPolicy, Telemetry, observe_requests};
pub use router::with_manifest;

#[doc(hidden)]
pub use tracing as __tracing;

#[macro_export]
macro_rules! request_telemetry {
    ($plugin:literal) => {
        $crate::server::Telemetry {
            span: |request_id, operation| {
                $crate::server::__tracing::debug_span!(
                    concat!($plugin, "_plugin_request"),
                    %request_id,
                    operation
                )
            },
            finished: |finished| {
                $crate::server::__tracing::debug!(
                    event = concat!($plugin, "_plugin_request_finished"),
                    outcome = finished.outcome,
                    status = finished.status,
                    elapsed_ms = finished.elapsed_ms
                )
            },
        }
    };
}
