use axum::{
    Router,
    extract::{Query, Request, State, rejection::QueryRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use subtle::ConstantTimeEq;
use uuid::Uuid;

pub(super) struct Access {
    authority: String,
    origin: String,
    ticket: Mutex<Option<String>>,
    cookie: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    ticket: String,
}

impl Access {
    pub fn new(origin: &url::Url) -> (Arc<Self>, url::Url) {
        let ticket = secret();
        let mut entry = origin.clone();
        entry.set_path("/__desktop/start");
        entry.query_pairs_mut().append_pair("ticket", &ticket);
        (
            Arc::new(Self {
                authority: origin.authority().into(),
                origin: origin.origin().ascii_serialization(),
                ticket: Mutex::new(Some(ticket)),
                cookie: secret(),
            }),
            entry,
        )
    }

    pub fn protect(self: Arc<Self>, app: Router) -> Router {
        Router::new()
            .route("/__desktop/start", get(bootstrap))
            .with_state(self.clone())
            .merge(app)
            .layer(middleware::from_fn_with_state(self, verify))
    }
}

fn secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn matches(actual: &str, expected: &str) -> bool {
    bool::from(actual.as_bytes().ct_eq(expected.as_bytes()))
}

async fn bootstrap(
    State(access): State<Arc<Access>>,
    start: Result<Query<Start>, QueryRejection>,
) -> Response {
    let Ok(Query(start)) = start else {
        tracing::debug!(
            event = "desktop_bootstrap_rejected",
            reason = "query",
            "Local window input rejected"
        );
        return (
            StatusCode::BAD_REQUEST,
            "Desktop access could not be verified.",
        )
            .into_response();
    };
    let mut ticket = access
        .ticket
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !ticket
        .as_ref()
        .is_some_and(|ticket| matches(&start.ticket, ticket))
    {
        return denied();
    }
    ticket.take();
    tracing::info!(
        event = "desktop_window_bootstrapped",
        "Local window access accepted"
    );
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, "/library".to_string()),
            (
                header::SET_COOKIE,
                format!(
                    "hito_desktop={}; Path=/; HttpOnly; SameSite=Strict",
                    access.cookie
                ),
            ),
        ],
    )
        .into_response()
}

async fn verify(State(access): State<Arc<Access>>, request: Request, next: Next) -> Response {
    let authority = request
        .headers()
        .get(header::HOST)
        .is_some_and(|host| host == access.authority.as_str());
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .is_none_or(|origin| origin == access.origin.as_str());
    let bootstrap = request.method() == "GET" && request.uri().path() == "/__desktop/start";
    let cookies = request
        .headers()
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .flat_map(|header| header.split(';'))
        .filter_map(|part| part.trim().strip_prefix("hito_desktop="))
        .collect::<Vec<_>>();
    let authenticated = cookies.len() == 1 && matches(cookies[0], &access.cookie);
    let mut response = if authority && origin && (bootstrap || authenticated) {
        next.run(request).await
    } else {
        tracing::warn!(
            event = "desktop_access_rejected",
            "Local window access rejected"
        );
        denied()
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    response
        .headers_mut()
        .insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    response
}

fn denied() -> Response {
    (
        StatusCode::FORBIDDEN,
        "Desktop access could not be verified.",
    )
        .into_response()
}
