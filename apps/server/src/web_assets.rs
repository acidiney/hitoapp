use crate::Config;
use axum::{
    Extension, Router,
    body::Body,
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use cap_fs_ext::OpenOptionsSyncExt;
use cap_std::fs::{Dir, OpenOptions};
use hito_core::Services;
use std::{io, time::Instant};
use tokio_util::io::ReaderStream;

#[derive(Clone, Copy)]
struct Asset {
    filename: &'static str,
    content_type: &'static str,
    service_worker: bool,
}

pub fn router() -> Router<Services> {
    let mut router = Router::new();
    for asset in [
        Asset {
            filename: "site.webmanifest",
            content_type: "application/manifest+json",
            service_worker: false,
        },
        Asset {
            filename: "sw.js",
            content_type: "text/javascript",
            service_worker: true,
        },
        Asset {
            filename: "favicon.ico",
            content_type: "image/x-icon",
            service_worker: false,
        },
        Asset {
            filename: "favicon.svg",
            content_type: "image/svg+xml",
            service_worker: false,
        },
        Asset {
            filename: "favicon-16.png",
            content_type: "image/png",
            service_worker: false,
        },
        Asset {
            filename: "favicon-32.png",
            content_type: "image/png",
            service_worker: false,
        },
        Asset {
            filename: "apple-touch-icon.png",
            content_type: "image/png",
            service_worker: false,
        },
        Asset {
            filename: "icon-192.png",
            content_type: "image/png",
            service_worker: false,
        },
        Asset {
            filename: "icon-512.png",
            content_type: "image/png",
            service_worker: false,
        },
        Asset {
            filename: "icon-maskable-512.png",
            content_type: "image/png",
            service_worker: false,
        },
        Asset {
            filename: "icon.svg",
            content_type: "image/svg+xml",
            service_worker: false,
        },
    ] {
        router = router.route(
            &format!("/{}", asset.filename),
            get(serve).layer(Extension(asset)),
        );
    }
    router
}

async fn serve(
    Extension(config): Extension<Config>,
    Extension(asset): Extension<Asset>,
    method: Method,
) -> Response {
    let started = Instant::now();
    let opened = tokio::task::spawn_blocking(move || -> io::Result<_> {
        let directory = Dir::open_ambient_dir(config.web_dir, cap_std::ambient_authority())?;
        let mut options = OpenOptions::new();
        options.read(true).nonblock(true);
        let file = directory.open_with(asset.filename, &options)?.into_std();
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok((file, metadata.len()))
    })
    .await;
    let (file, length) = match opened {
        Ok(Ok(opened)) => opened,
        _ => {
            tracing::debug!(
                event = "web_asset",
                outcome = "not_found",
                duration_ms = started.elapsed().as_millis() as u64,
                "Web asset unavailable"
            );
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };
    tracing::debug!(
        event = "web_asset",
        outcome = "opened",
        duration_ms = started.elapsed().as_millis() as u64,
        "Web asset opened"
    );
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from_stream(ReaderStream::new(tokio::fs::File::from_std(file)))
    };
    let mut response = (
        [
            (header::CONTENT_TYPE, asset.content_type.to_owned()),
            (header::CONTENT_LENGTH, length.to_string()),
        ],
        body,
    )
        .into_response();
    if asset.service_worker {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
        response
            .headers_mut()
            .insert("service-worker-allowed", "/".parse().unwrap());
    }
    response
}
