pub use hito_testkit::tone_files as tones;

use hito_core::test_support::support;
use hito_core::{
    Actor, AudioRequest, CoreConfig, Error, QualityAudio, QualityRequest, Search, Services,
    StreamQuality,
};
use std::{path::Path, time::Duration};

pub struct Library {
    pub directory: tempfile::TempDir,
    _tools: tempfile::TempDir,
    pub services: Services,
    pub actor: Actor,
    pub source: String,
}

pub async fn library(
    seconds: u32,
    extension: &str,
    write: impl FnOnce(&Path, u32),
    configure: impl FnOnce(&mut CoreConfig, &Path),
) -> Library {
    let tools = tempfile::tempdir().unwrap();
    let (directory, services, actor, library, music) =
        support::library_configured("conversion", |config| {
            configure(config, tools.path());
        })
        .await;
    write(&music.join(format!("track.{extension}")), seconds);
    support::scan(&services, &actor, &library).await;
    let recording = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let source = services
        .catalog
        .sources(&actor, &recording)
        .await
        .unwrap()
        .remove(0)
        .id;
    Library {
        directory,
        _tools: tools,
        services,
        actor,
        source,
    }
}

pub fn flac(path: &Path, seconds: u32) {
    tones::flac(path, seconds);
}

pub fn no_configuration(_: &mut CoreConfig, _: &Path) {}

pub fn request(quality: StreamQuality, start_seconds: u64) -> QualityRequest {
    QualityRequest {
        quality,
        start: Duration::from_secs(start_seconds),
        original: AudioRequest::default(),
    }
}

pub async fn open(library: &Library, request: QualityRequest) -> Result<QualityAudio, Error> {
    library
        .services
        .playback
        .open_quality_audio(&library.actor, &library.source, request)
        .await
}

pub async fn converted_bytes(audio: QualityAudio) -> Vec<u8> {
    let QualityAudio::Converted(mut converted) = audio else {
        panic!("the source must be converted");
    };
    let mut body = converted.body.take().unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = body.chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    bytes
}
