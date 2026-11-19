mod audio;
mod server;

use audio::RemoteCandidates;
use hito_core::{
    Artwork, AudiobookChapterAnchor, AudiobookPlayback, Error, PrepareSource, Recording,
    RecordingProgress, ResolveSource, Result, SavedPosition, Source, SourcePreparationReceipt,
    SourcePreparationStatus, SourceResolution, SyncBatch, SyncReceipt, User,
};
use hito_playback::seam::{
    Admission, Authorization, Call, Candidates, CatalogReads, Listener, Progress, Seam, Sources,
    Stop,
};
use hito_server::{Identified, Identify};
use reqwest::{Method, StatusCode, header};
use serde_json::json;
pub use server::Server;
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

const LOOKUP_LIMIT: usize = 500;
const CHAPTER_LIMIT: usize = 1000;
const SESSION_TRUST: Duration = Duration::from_secs(2);

pub fn seam(server: Arc<Server>) -> Seam {
    let remote = Arc::new(Remote {
        server,
        checked: Mutex::new(None),
    });
    Seam {
        authorization: remote.clone(),
        catalog: remote.clone(),
        progress: remote.clone(),
        sources: remote,
    }
}

struct Remote {
    server: Arc<Server>,
    checked: Mutex<Option<Instant>>,
}

struct Published;

impl Admission for Published {
    fn publish(self: Box<Self>, publish: Box<dyn FnOnce() + '_>) -> Result<()> {
        publish();
        Ok(())
    }
}

impl Remote {
    async fn check_chapters(&self, chapters: &[AudiobookChapterAnchor]) -> Result<()> {
        let request = self
            .server
            .request(Method::POST, &["audiobooks", "chapters", "check"])?
            .json(&json!({ "chapters": chapters }));
        self.server.send(request).await.map(|_| ())
    }

    async fn artwork_at(&self, segments: &[&str]) -> Result<Option<Artwork>> {
        let request = self.server.request(Method::GET, segments)?;
        let response = match self.server.send(request).await {
            Ok(response) => response,
            Err(Error::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        let content_type = match response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
        {
            Some("image/png") => "image/png",
            Some("image/jpeg") => "image/jpeg",
            Some("image/gif") => "image/gif",
            Some("image/webp") => "image/webp",
            _ => return Ok(None),
        };
        let bytes = response
            .bytes()
            .await
            .map_err(|_| Error::Upstream("The server stopped sending artwork"))?;
        Ok(Some(Artwork {
            content_type,
            bytes: bytes.to_vec(),
        }))
    }
}

impl Authorization for Remote {
    fn check_session<'a>(&'a self, _actor: &'a Listener) -> Call<'a, ()> {
        Box::pin(async move {
            let recent = self
                .checked
                .lock()
                .unwrap()
                .is_some_and(|checked| checked.elapsed() < SESSION_TRUST);
            if recent {
                return Ok(());
            }
            let request = self.server.request(Method::GET, &["me"])?;
            self.server.send(request).await?;
            *self.checked.lock().unwrap() = Some(Instant::now());
            Ok(())
        })
    }

    fn check_audiobook_chapters<'a>(
        &'a self,
        _actor: &'a Listener,
        chapters: &'a [AudiobookChapterAnchor],
    ) -> Call<'a, ()> {
        Box::pin(async move {
            for chunk in chapters.chunks(CHAPTER_LIMIT) {
                self.check_chapters(chunk).await?;
            }
            Ok(())
        })
    }

    fn admit_audiobook<'a>(
        &'a self,
        _actor: &'a Listener,
        target: AudiobookChapterAnchor,
        chapters: &'a [AudiobookChapterAnchor],
    ) -> Call<'a, Box<dyn Admission>> {
        Box::pin(async move {
            let mut admitted = false;
            for chunk in chapters.chunks(CHAPTER_LIMIT) {
                if !chunk.contains(&target) {
                    self.check_chapters(chunk).await?;
                    continue;
                }
                let book = target.audiobook_id.to_string();
                let request = self
                    .server
                    .request(Method::POST, &["audiobooks", &book, "admission"])?
                    .json(&json!({ "target": target, "chapters": chunk }));
                self.server.send(request).await?;
                admitted = true;
            }
            if !admitted {
                return Err(Error::Invalid(
                    "The audiobook playback selection is invalid".into(),
                ));
            }
            Ok(Box::new(Published) as Box<dyn Admission>)
        })
    }
}

impl CatalogReads for Remote {
    fn recordings<'a>(&'a self, _actor: &'a Listener, ids: &'a [Uuid]) -> Call<'a, Vec<Recording>> {
        Box::pin(async move {
            let mut found: Vec<Recording> = Vec::with_capacity(ids.len());
            for chunk in ids.chunks(LOOKUP_LIMIT) {
                let request = self
                    .server
                    .request(Method::POST, &["recordings", "lookup"])?
                    .json(&json!({ "recording_ids": chunk }));
                found.extend(self.server.json::<Vec<Recording>>(request).await?);
            }
            ids.iter()
                .map(|id| {
                    let id = id.to_string();
                    found
                        .iter()
                        .position(|recording| recording.id == id)
                        .map(|index| found.swap_remove(index))
                        .ok_or(Error::NotFound)
                })
                .collect()
        })
    }

    fn release_artwork<'a>(
        &'a self,
        _actor: &'a Listener,
        release: &'a str,
    ) -> Call<'a, Option<Artwork>> {
        Box::pin(async move { self.artwork_at(&["releases", release, "artwork"]).await })
    }

    fn artwork<'a>(
        &'a self,
        _actor: &'a Listener,
        recording: &'a str,
    ) -> Call<'a, Option<Artwork>> {
        Box::pin(async move { self.artwork_at(&["recordings", recording, "artwork"]).await })
    }

    fn audiobook_playback<'a>(
        &'a self,
        _actor: &'a Listener,
        audiobook: &'a str,
    ) -> Call<'a, AudiobookPlayback> {
        Box::pin(async move {
            let request = self
                .server
                .request(Method::GET, &["audiobooks", audiobook, "playback"])?;
            self.server.json(request).await
        })
    }
}

impl Progress for Remote {
    fn saved_position<'a>(
        &'a self,
        _actor: &'a Listener,
        recording: Uuid,
    ) -> Call<'a, Option<SavedPosition>> {
        Box::pin(async move {
            let request = self
                .server
                .request(Method::POST, &["recordings", "progress"])?
                .json(&json!({ "recording_ids": [recording] }));
            let saved: Vec<RecordingProgress> = self.server.json(request).await?;
            Ok(saved
                .into_iter()
                .find(|progress| progress.recording_id == recording)
                .map(|progress| SavedPosition {
                    position_ms: progress.position_ms,
                }))
        })
    }

    fn apply<'a>(&'a self, _actor: &'a Listener, batch: SyncBatch) -> Call<'a, SyncReceipt> {
        Box::pin(async move {
            let request = self.server.request(Method::POST, &["sync"])?.json(&batch);
            self.server.json(request).await
        })
    }
}

impl Sources for Remote {
    fn candidates<'a>(
        &'a self,
        _actor: &'a Listener,
        recording: &'a str,
    ) -> Call<'a, Box<dyn Candidates>> {
        Box::pin(async move {
            let request = self
                .server
                .request(Method::GET, &["recordings", recording, "playback-sources"])?;
            let sources: Vec<Source> = self.server.json(request).await?;
            Ok(Box::new(RemoteCandidates::new(
                self.server.clone(),
                recording,
                sources,
            )) as Box<dyn Candidates>)
        })
    }

    fn refresh_until<'a>(
        &'a self,
        _actor: &'a Listener,
        recording: &'a str,
        stop: Stop<'a>,
    ) -> Call<'a, bool> {
        Box::pin(async move {
            let request = self
                .server
                .request(
                    Method::POST,
                    &["recordings", recording, "sources", "refresh"],
                )?
                .json(&json!({ "purpose": "playback" }));
            tokio::select! {
                biased;
                _ = stop => Ok(false),
                result = self.server.send(request) => result.map(|_| true),
            }
        })
    }

    fn resolve_until<'a>(
        &'a self,
        _actor: &'a Listener,
        recording: &'a str,
        request: ResolveSource,
        stop: Stop<'a>,
    ) -> Call<'a, SourceResolution> {
        Box::pin(async move {
            let request = self
                .server
                .request(
                    Method::POST,
                    &["recordings", recording, "sources", "resolve"],
                )?
                .json(&request);
            tokio::select! {
                biased;
                _ = stop => Err(Error::Upstream("Finding audio was cancelled")),
                result = self.server.json(request) => result,
            }
        })
    }

    fn sources<'a>(&'a self, _actor: &'a Listener, recording: &'a str) -> Call<'a, Vec<Source>> {
        Box::pin(async move {
            let request = self
                .server
                .request(Method::GET, &["recordings", recording, "sources"])?;
            self.server.json(request).await
        })
    }

    fn prepare<'a>(
        &'a self,
        _actor: &'a Listener,
        recording: &'a str,
        source: &'a str,
        request: PrepareSource,
    ) -> Call<'a, SourcePreparationReceipt> {
        Box::pin(async move {
            let request = self
                .server
                .request(
                    Method::POST,
                    &["recordings", recording, "sources", source, "preparation"],
                )?
                .json(&request);
            self.server.json(request).await
        })
    }

    fn preparation<'a>(
        &'a self,
        _actor: &'a Listener,
        recording: &'a str,
        source: &'a str,
    ) -> Call<'a, SourcePreparationStatus> {
        Box::pin(async move {
            let request = self.server.request(
                Method::GET,
                &["recordings", recording, "sources", source, "preparation"],
            )?;
            self.server.json(request).await
        })
    }
}

pub struct RemoteIdentity {
    server: Arc<Server>,
    known: Mutex<Option<(String, User)>>,
}

impl RemoteIdentity {
    pub fn new(server: Arc<Server>) -> Self {
        Self {
            server,
            known: Mutex::new(None),
        }
    }
}

fn session_key(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl Identify for RemoteIdentity {
    fn identify<'a>(&'a self, _parts: &'a mut axum::http::request::Parts) -> Identified<'a> {
        Box::pin(async move {
            let token = self.server.token().ok_or(Error::Unauthorized)?;
            let key = session_key(&token);
            let known = self.known.lock().unwrap().clone();
            if let Some((known_key, user)) = known
                && known_key == key
            {
                return Ok(Listener::remote(user, key));
            }
            let request = self.server.request(Method::GET, &["me"])?;
            let response = self.server.send(request).await?;
            if response.status() != StatusCode::OK {
                return Err(Error::Unauthorized.into());
            }
            let user: User = response
                .json()
                .await
                .map_err(|_| Error::Upstream("The server sent an answer this app cannot read"))?;
            *self.known.lock().unwrap() = Some((key.clone(), user.clone()));
            Ok(Listener::remote(user, key))
        })
    }
}
