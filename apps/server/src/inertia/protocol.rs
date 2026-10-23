use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

pub struct Protocol {
    pub inertia: bool,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

impl Protocol {
    pub fn new(headers: &HeaderMap, component: &str) -> Self {
        let inertia = headers
            .get("x-inertia")
            .is_some_and(|value| value == "true");
        let partial = inertia
            && headers
                .get("x-inertia-partial-component")
                .is_some_and(|value| value == component);
        let keys = |header| {
            partial
                .then(|| headers.get(header).and_then(|value| value.to_str().ok()))
                .flatten()
                .map(|value| value.split(',').map(|key| key.trim().to_string()).collect())
        };
        Self {
            inertia,
            include: keys("x-inertia-partial-data"),
            exclude: keys("x-inertia-partial-except"),
        }
    }

    pub fn requested(&self, key: &str) -> bool {
        self.include
            .as_ref()
            .is_none_or(|keys| keys.iter().any(|item| item == key))
            && self
                .exclude
                .as_ref()
                .is_none_or(|keys| !keys.iter().any(|item| item == key))
    }

    pub fn version_conflict(headers: &HeaderMap, version: &str, url: &str) -> Option<Response> {
        if headers
            .get("x-inertia")
            .is_some_and(|value| value == "true")
            && headers
                .get("x-inertia-version")
                .is_some_and(|value| value != version)
        {
            tracing::info!(
                event = "inertia_version_mismatch",
                "Client assets require a reload"
            );
            Some(
                (
                    StatusCode::CONFLICT,
                    [
                        ("x-inertia-location", url.to_string()),
                        ("vary", "X-Inertia".into()),
                    ],
                )
                    .into_response(),
            )
        } else {
            None
        }
    }
}
