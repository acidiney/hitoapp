use crate::ServerAddress;
use reqwest::{Client, StatusCode, header::LOCATION};
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    Found(Url),
    NoAnswer,
    NotHito(String),
    Certificate(String),
}

enum Attempt {
    Silent,
    Answered(Probe),
}

pub struct Prober {
    client: Client,
}

impl Prober {
    pub fn new() -> reqwest::Result<Self> {
        Self::with_roots(&[])
    }

    pub fn with_roots(extra_roots: &[&[u8]]) -> reqwest::Result<Self> {
        tls_ready();
        let mut builder = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(8))
            .retry(reqwest::retry::never())
            .user_agent(concat!("Hito-desktop/", env!("CARGO_PKG_VERSION")));
        for root in extra_roots {
            builder = builder.add_root_certificate(reqwest::Certificate::from_der(root)?);
        }
        Ok(Self {
            client: builder.build()?,
        })
    }

    pub async fn probe(&self, address: &ServerAddress) -> Probe {
        for candidate in address.candidates() {
            match self.attempt(&candidate, true).await {
                Attempt::Answered(probe) => return probe,
                Attempt::Silent => continue,
            }
        }
        Probe::NoAnswer
    }

    async fn attempt(&self, origin: &Url, follow_upgrade: bool) -> Attempt {
        let host = origin.host_str().unwrap_or_default().to_owned();
        let Ok(health) = origin.join("health") else {
            return Attempt::Silent;
        };
        let response = match self.client.get(health).send().await {
            Ok(response) => response,
            Err(error) if certificate_problem(&error) => {
                return Attempt::Answered(Probe::Certificate(host));
            }
            Err(_) => return Attempt::Silent,
        };
        if response.status().is_redirection() {
            let target = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| origin.join(value).ok());
            return match target {
                Some(target)
                    if follow_upgrade
                        && origin.scheme() == "http"
                        && target.scheme() == "https"
                        && target.host_str() == origin.host_str() =>
                {
                    let mut upgraded = target.clone();
                    upgraded.set_path("/");
                    upgraded.set_query(None);
                    match Box::pin(self.attempt(&upgraded, false)).await {
                        Attempt::Answered(probe) => Attempt::Answered(probe),
                        Attempt::Silent => Attempt::Answered(Probe::NoAnswer),
                    }
                }
                _ => Attempt::Answered(Probe::NotHito(host)),
            };
        }
        if !matches!(
            response.status(),
            StatusCode::OK | StatusCode::SERVICE_UNAVAILABLE
        ) {
            return Attempt::Answered(Probe::NotHito(host));
        }
        let reports_health = response
            .json::<serde_json::Value>()
            .await
            .is_ok_and(|body| body.get("ready").is_some_and(serde_json::Value::is_boolean));
        if reports_health {
            Attempt::Answered(Probe::Found(origin.clone()))
        } else {
            Attempt::Answered(Probe::NotHito(host))
        }
    }
}

pub(crate) fn tls_ready() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

fn certificate_problem(error: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        let mut layer: &(dyn std::error::Error + 'static) = current;
        while let Some(inner) = layer
            .downcast_ref::<std::io::Error>()
            .and_then(|io| io.get_ref())
        {
            layer = inner;
        }
        if let Some(rustls::Error::InvalidCertificate(_)) = layer.downcast_ref::<rustls::Error>() {
            return true;
        }
        source = current.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        http::{HeaderValue, StatusCode as AxumStatus},
        response::IntoResponse,
        routing::get,
    };
    use hito_testkit::tls_api::{Fixture, Reply};
    use tokio::net::TcpListener;

    async fn plain(router: Router) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        port
    }

    fn hito() -> Router {
        Router::new().route(
            "/health",
            get(|| async { axum::Json(serde_json::json!({"ready": true})) }),
        )
    }

    fn address(typed: &str) -> ServerAddress {
        ServerAddress::parse(typed).unwrap()
    }

    #[tokio::test]
    async fn without_a_scheme_a_plain_http_server_is_found_after_https_gets_no_answer() {
        let port = plain(hito()).await;
        let probe = Prober::new()
            .unwrap()
            .probe(&address(&format!("127.0.0.1:{port}")))
            .await;
        assert_eq!(
            probe,
            Probe::Found(Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap())
        );
    }

    #[tokio::test]
    async fn a_typed_https_is_never_replaced_by_http() {
        let port = plain(hito()).await;
        let probe = Prober::new()
            .unwrap()
            .probe(&address(&format!("https://127.0.0.1:{port}")))
            .await;
        assert_eq!(probe, Probe::NoAnswer);
    }

    #[tokio::test]
    async fn a_certificate_that_cannot_be_checked_has_no_http_fallback() {
        let fixture = Fixture::start("localhost", vec![]).await;
        let port = fixture.address().port();
        let probe = Prober::new()
            .unwrap()
            .probe(&address(&format!("localhost:{port}")))
            .await;
        assert_eq!(probe, Probe::Certificate("localhost".into()));
        fixture.close().await;
    }

    #[tokio::test]
    async fn once_https_answers_its_failure_is_reported_without_trying_http() {
        let mut failure = Reply::json(b"{}".to_vec());
        failure.status = 500;
        let fixture = Fixture::responses("localhost", vec![failure]).await;
        let port = fixture.address().port();
        let prober = Prober::with_roots(&[fixture.ca_der()]).unwrap();
        let probe = prober.probe(&address(&format!("localhost:{port}"))).await;
        assert_eq!(probe, Probe::NotHito("localhost".into()));
        assert_eq!(fixture.close().await.len(), 1);
    }

    #[tokio::test]
    async fn a_trusted_https_server_is_found_first() {
        let fixture = Fixture::start("localhost", vec![br#"{"ready":true}"#.to_vec()]).await;
        let port = fixture.address().port();
        let prober = Prober::with_roots(&[fixture.ca_der()]).unwrap();
        let probe = prober.probe(&address(&format!("localhost:{port}"))).await;
        assert_eq!(
            probe,
            Probe::Found(Url::parse(&format!("https://localhost:{port}/")).unwrap())
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn something_other_than_hito_is_reported_as_not_hito() {
        let port =
            plain(Router::new().route("/health", get(|| async { "<html>router</html>" }))).await;
        let probe = Prober::new()
            .unwrap()
            .probe(&address(&format!("http://127.0.0.1:{port}")))
            .await;
        assert_eq!(probe, Probe::NotHito("127.0.0.1".into()));
    }

    #[tokio::test]
    async fn a_redirect_to_another_host_is_not_hito() {
        let port = plain(Router::new().route(
            "/health",
            get(|| async {
                let mut response = AxumStatus::FOUND.into_response();
                response.headers_mut().insert(
                    "location",
                    HeaderValue::from_static("https://elsewhere.example/health"),
                );
                response
            }),
        ))
        .await;
        let probe = Prober::new()
            .unwrap()
            .probe(&address(&format!("http://127.0.0.1:{port}")))
            .await;
        assert_eq!(probe, Probe::NotHito("127.0.0.1".into()));
    }

    #[tokio::test]
    async fn a_hito_server_that_is_still_starting_is_found() {
        let port = plain(Router::new().route(
            "/health",
            get(|| async {
                (
                    AxumStatus::SERVICE_UNAVAILABLE,
                    axum::Json(serde_json::json!({"ready": false})),
                )
            }),
        ))
        .await;
        let probe = Prober::new()
            .unwrap()
            .probe(&address(&format!("http://127.0.0.1:{port}")))
            .await;
        assert_eq!(
            probe,
            Probe::Found(Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap())
        );
    }

    #[tokio::test]
    async fn a_plain_server_that_upgrades_to_https_on_the_same_host_is_checked_there() {
        let fixture = Fixture::start("localhost", vec![br#"{"ready":true}"#.to_vec()]).await;
        let secure = fixture.address().port();
        let port = plain(Router::new().route(
            "/health",
            get(move || async move {
                let mut response = AxumStatus::PERMANENT_REDIRECT.into_response();
                let location = format!("https://localhost:{secure}/health");
                response
                    .headers_mut()
                    .insert("location", HeaderValue::from_str(&location).unwrap());
                response
            }),
        ))
        .await;
        let prober = Prober::with_roots(&[fixture.ca_der()]).unwrap();
        let probe = prober
            .probe(&address(&format!("http://localhost:{port}")))
            .await;
        assert_eq!(
            probe,
            Probe::Found(Url::parse(&format!("https://localhost:{secure}/")).unwrap())
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn a_closed_port_has_no_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let probe = Prober::new()
            .unwrap()
            .probe(&address(&format!("127.0.0.1:{port}")))
            .await;
        assert_eq!(probe, Probe::NoAnswer);
    }
}
