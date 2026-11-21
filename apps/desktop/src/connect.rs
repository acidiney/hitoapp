use crate::{Probe, Prober, RecentServers, ServerAddress};
use serde::Serialize;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Opened,
    NotAnAddress,
    NoAnswer { shown: String },
    NotHito { host: String },
    Certificate { host: String },
    ServerTooOld { version: Option<String> },
    AppTooOld,
    NotEncrypted { origin: String, shown: String },
    Busy,
    Failed,
}

impl Outcome {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::NotAnAddress => "not_an_address",
            Self::NoAnswer { .. } => "no_answer",
            Self::NotHito { .. } => "not_hito",
            Self::Certificate { .. } => "certificate",
            Self::ServerTooOld { .. } => "server_too_old",
            Self::AppTooOld => "app_too_old",
            Self::NotEncrypted { .. } => "not_encrypted",
            Self::Busy => "busy",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    Open(Url),
    Answer(Outcome),
}

pub async fn check(
    prober: &Prober,
    typed: &str,
    recent: &RecentServers,
    accept_plain: bool,
) -> Next {
    let Ok(address) = ServerAddress::parse(typed) else {
        return Next::Answer(Outcome::NotAnAddress);
    };
    match prober.probe(&address).await {
        Probe::Found(origin) => {
            let accepted = accept_plain
                || recent
                    .find(&origin)
                    .is_some_and(|server| server.plain_accepted);
            if origin.scheme() == "http" && !accepted {
                Next::Answer(Outcome::NotEncrypted {
                    shown: shown(&origin),
                    origin: origin.to_string(),
                })
            } else {
                Next::Open(origin)
            }
        }
        Probe::NoAnswer => Next::Answer(Outcome::NoAnswer {
            shown: address.shown(),
        }),
        Probe::NotHito(host) => Next::Answer(Outcome::NotHito { host }),
        Probe::Certificate(host) => Next::Answer(Outcome::Certificate { host }),
        Probe::ServerTooOld { version, .. } => Next::Answer(Outcome::ServerTooOld { version }),
        Probe::AppTooOld(_) => Next::Answer(Outcome::AppTooOld),
    }
}

pub fn shown(origin: &Url) -> String {
    origin.origin().ascii_serialization()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};
    use tokio::net::TcpListener;

    async fn hito() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = Router::new().route(
            "/health",
            get(|| async {
                axum::Json(
                    serde_json::json!({"ready": true, "api_level": 1, "min_client_api_level": 1}),
                )
            }),
        );
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        port
    }

    #[tokio::test]
    async fn a_plain_server_asks_first_and_opens_once_the_person_agrees() {
        let port = hito().await;
        let prober = Prober::new().unwrap();
        let typed = format!("127.0.0.1:{port}");
        let origin = format!("http://127.0.0.1:{port}/");
        let recent = RecentServers::default();
        assert_eq!(
            check(&prober, &typed, &recent, false).await,
            Next::Answer(Outcome::NotEncrypted {
                origin: origin.clone(),
                shown: format!("http://127.0.0.1:{port}"),
            })
        );
        assert_eq!(
            check(&prober, &origin, &recent, true).await,
            Next::Open(Url::parse(&origin).unwrap())
        );
    }

    #[tokio::test]
    async fn the_notice_is_shown_once_per_server() {
        let port = hito().await;
        let prober = Prober::new().unwrap();
        let origin = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let mut recent = RecentServers::default();
        recent.accept_plain(&origin, 1);
        assert_eq!(
            check(&prober, origin.as_str(), &recent, false).await,
            Next::Open(origin)
        );
    }

    #[tokio::test]
    async fn an_address_that_cannot_be_read_is_refused_before_looking() {
        let prober = Prober::new().unwrap();
        assert_eq!(
            check(&prober, "my server", &RecentServers::default(), false).await,
            Next::Answer(Outcome::NotAnAddress)
        );
    }

    #[tokio::test]
    async fn no_answer_names_the_address_as_the_person_typed_it() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let prober = Prober::new().unwrap();
        assert_eq!(
            check(
                &prober,
                &format!("127.0.0.1:{port}/library"),
                &RecentServers::default(),
                false
            )
            .await,
            Next::Answer(Outcome::NoAnswer {
                shown: format!("127.0.0.1:{port}"),
            })
        );
    }
}
