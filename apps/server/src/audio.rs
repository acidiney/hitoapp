mod original;

pub(crate) use original::lease_response;

use crate::error::ApiResult;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use hito_core::{Actor, AudioRequest, PlaybackAccess, QualityAudio, QualityRequest, StreamQuality};
use serde::Deserialize;
use std::time::{Duration, Instant};
use tracing::Instrument;
use utoipa::IntoParams;

#[derive(Default, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct AudioQuery {
    #[serde(default)]
    pub quality: StreamQuality,
    #[serde(default)]
    #[param(maximum = 604800000)]
    pub start_ms: u64,
}

#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str(), quality = query.quality.label()))]
pub async fn response(
    playback: &PlaybackAccess,
    actor: &Actor,
    source: &str,
    query: AudioQuery,
    request: Request<Body>,
) -> ApiResult<Response> {
    let started = Instant::now();
    let quality_request = QualityRequest {
        quality: query.quality,
        start: Duration::from_millis(query.start_ms),
        original: AudioRequest {
            head: request.method() == Method::HEAD,
            range: None,
        },
    };
    quality_request.validate()?;
    if query.quality == StreamQuality::Original {
        if query.start_ms > 0 {
            return Err(hito_core::Error::Invalid(
                "Start position requires a converted stream".into(),
            )
            .into());
        }
        return original::response(playback, actor, source, request).await;
    }
    let opened = playback
        .open_quality_audio(actor, source, quality_request)
        .await?;
    match opened {
        QualityAudio::Original { audio, reason } => {
            tracing::debug!(
                event = "audio_quality_response",
                outcome = if quality_request.original.head {
                    "original"
                } else {
                    "retry_original"
                },
                reason,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Audio quality negotiated"
            );
            if !quality_request.original.head {
                drop(audio);
                return Err(hito_core::Error::Conflict(
                    "Selected audio quality is unavailable; retry the original stream".into(),
                )
                .into());
            }
            original::content(audio, false)
        }
        QualityAudio::Converted(mut audio) => {
            let body = audio
                .body
                .take()
                .map(|stream| {
                    let span = tracing::Span::current();
                    Body::from_stream(futures_util::stream::try_unfold(
                        stream,
                        move |mut stream| {
                            let span = span.clone();
                            async move {
                                stream
                                    .chunk()
                                    .await
                                    .map(|chunk| chunk.map(|bytes| (bytes, stream)))
                                    .map_err(|error| {
                                        tracing::debug!(
                                            event = "audio_quality_body_failed",
                                            error_kind = error.code(),
                                            "Converted audio delivery failed"
                                        );
                                        std::io::Error::other("Audio stream could not finish")
                                    })
                            }
                            .instrument(span)
                        },
                    ))
                })
                .unwrap_or_else(|| {
                    Body::from_stream(futures_util::stream::empty::<
                        Result<axum::body::Bytes, std::io::Error>,
                    >())
                });
            tracing::debug!(
                event = "audio_quality_response",
                outcome = "converted",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Audio quality negotiated"
            );
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, audio.content_type)
                .header(header::CACHE_CONTROL, "private, no-store")
                .header("x-hito-stream-quality", query.quality.label())
                .header(
                    "x-hito-stream-start-ms",
                    audio.start.as_millis().to_string(),
                )
                .body(body)
                .unwrap())
        }
    }
}
