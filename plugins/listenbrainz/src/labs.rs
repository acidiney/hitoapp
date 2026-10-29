use crate::{
    config::{Config, InitError},
    limiter::Limiter,
};
use hito_plugin_sdk::{
    ArtistSimilarityPage, ArtistSimilarityRequest, CatalogArtistRef, SimilarArtist,
};
use serde::Deserialize;
use std::{collections::HashSet, time::Duration};
use url::Url;
use uuid::Uuid;

const MAX_BODY_BYTES: usize = 262_144;
const ALGORITHM: &str =
    "session_based_days_9000_session_300_contribution_5_threshold_15_limit_50_skip_30";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Invalid,
    Unavailable,
    BadResponse,
}

#[derive(Deserialize)]
struct Row {
    artist_mbid: String,
    name: String,
    score: Option<f64>,
}

#[derive(Clone)]
pub(crate) struct Labs {
    client: reqwest::Client,
    endpoint: Url,
    limiter: Limiter,
}

impl Labs {
    pub(crate) fn new(config: Config) -> Result<Self, InitError> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .user_agent(format!(
                "Hito/{} ListenBrainz-similarity-plugin ({})",
                env!("CARGO_PKG_VERSION"),
                config.contact
            ))
            .build()
            .map_err(|_| InitError::HttpClient)?;
        Ok(Self {
            client,
            endpoint: config.endpoint,
            limiter: Limiter::new(config.min_interval),
        })
    }

    pub(crate) async fn similar(
        &self,
        request: &ArtistSimilarityRequest,
    ) -> Result<ArtistSimilarityPage, Failure> {
        request.validate().map_err(|_| Failure::Invalid)?;
        if !canonical(&request.artist_id) {
            return Err(Failure::Invalid);
        }
        let mut url = self
            .endpoint
            .join("similar-artists/json")
            .map_err(|_| Failure::Invalid)?;
        url.query_pairs_mut()
            .append_pair("artist_mbids", &request.artist_id)
            .append_pair("algorithm", ALGORITHM);
        self.limiter
            .reserve()
            .await
            .map_err(|()| Failure::Unavailable)?;
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| Failure::Unavailable)?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(ArtistSimilarityPage {
                artist_id: request.artist_id.clone(),
                items: Vec::new(),
            });
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let wait = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<u64>().ok())
                .unwrap_or(10)
                .clamp(1, 300);
            self.limiter.back_off(Duration::from_secs(wait)).await;
            return Err(Failure::Unavailable);
        }
        if !status.is_success() {
            return Err(Failure::Unavailable);
        }
        let json = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
        if !json {
            return Err(Failure::BadResponse);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Failure::Unavailable)? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(Failure::BadResponse);
            }
            body.extend_from_slice(&chunk);
        }
        let rows: Vec<Row> = serde_json::from_slice(&body).map_err(|_| Failure::BadResponse)?;
        let mut seen = HashSet::new();
        let items = rows
            .into_iter()
            .filter(|row| {
                canonical(&row.artist_mbid)
                    && row.artist_mbid != request.artist_id
                    && !row.name.trim().is_empty()
                    && row.name.len() <= 2048
                    && !row.name.chars().any(char::is_control)
                    && row
                        .score
                        .is_none_or(|score| score.is_finite() && (0.0..=1.0e9).contains(&score))
                    && seen.insert(row.artist_mbid.clone())
            })
            .take(usize::from(request.limit))
            .map(|row| SimilarArtist {
                artist: CatalogArtistRef {
                    id: row.artist_mbid,
                    name: row.name,
                },
                score: row.score,
            })
            .collect();
        let page = ArtistSimilarityPage {
            artist_id: request.artist_id.clone(),
            items,
        };
        page.validate_for(request)
            .map_err(|_| Failure::BadResponse)?;
        Ok(page)
    }
}

fn canonical(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == value)
}
