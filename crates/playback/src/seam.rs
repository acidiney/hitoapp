use bytes::Bytes;
use hito_core::{
    Actor, Artwork, AudioFile, AudioRequest, AudiobookChapterAnchor, AudiobookPlayback,
    AudiobookPlaybackAdmission, CandidatesEnd, Error, OpenedAudio, PcmOpening, PlaybackCandidates,
    PlaybackSource, PrepareSource, Recording, ResolveSource, Result, SavedPosition, Services,
    Source, SourcePreparationReceipt, SourcePreparationStatus, SourceResolution, SyncBatch,
    SyncReceipt,
};
use std::{future::Future, ops::Deref, ops::RangeInclusive, pin::Pin, sync::Arc, time::Duration};
use uuid::Uuid;

pub type Call<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
pub type Stop<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

pub trait Authorization: Send + Sync {
    fn check_session<'a>(&'a self, actor: &'a Actor) -> Call<'a, ()>;
    fn check_audiobook_chapters<'a>(
        &'a self,
        actor: &'a Actor,
        chapters: &'a [AudiobookChapterAnchor],
    ) -> Call<'a, ()>;
    fn admit_audiobook<'a>(
        &'a self,
        actor: &'a Actor,
        target: AudiobookChapterAnchor,
        chapters: &'a [AudiobookChapterAnchor],
    ) -> Call<'a, Box<dyn Admission>>;
}

pub trait Admission: Send {
    fn publish(self: Box<Self>, publish: Box<dyn FnOnce() + '_>) -> Result<()>;
}

pub trait CatalogReads: Send + Sync {
    fn recordings<'a>(&'a self, actor: &'a Actor, ids: &'a [Uuid]) -> Call<'a, Vec<Recording>>;
    fn release_artwork<'a>(
        &'a self,
        actor: &'a Actor,
        release: &'a str,
    ) -> Call<'a, Option<Artwork>>;
    fn artwork<'a>(&'a self, actor: &'a Actor, recording: &'a str) -> Call<'a, Option<Artwork>>;
    fn audiobook_playback<'a>(
        &'a self,
        actor: &'a Actor,
        audiobook: &'a str,
    ) -> Call<'a, AudiobookPlayback>;
}

pub trait Progress: Send + Sync {
    fn saved_position<'a>(
        &'a self,
        actor: &'a Actor,
        recording: Uuid,
    ) -> Call<'a, Option<SavedPosition>>;
    fn apply<'a>(&'a self, actor: &'a Actor, batch: SyncBatch) -> Call<'a, SyncReceipt>;
}

pub trait Candidates: Send {
    fn ended(&self) -> Option<CandidatesEnd>;
    fn next(&mut self) -> Call<'_, Option<Grant>>;
}

pub trait Chunks: Send + Sync {
    fn chunk(&mut self) -> Call<'_, Option<Bytes>>;
}

pub struct Content {
    pub total_length: Option<u64>,
    pub range: Option<RangeInclusive<u64>>,
    pub seekable: bool,
    pub body: Box<dyn Chunks>,
}

pub enum Opened {
    Content(Content),
    Unsatisfiable { length: Option<u64> },
}

pub trait AudioAccess: Send + Sync {
    fn source_id(&self) -> &str;
    fn is_local(&self) -> bool;
    fn check(&self) -> Call<'_, ()>;
    fn revoked(&self) -> Pin<Box<dyn Future<Output = Error> + Send + '_>>;
    fn open_file(&self) -> Call<'_, AudioFile>;
    fn open(&self, request: AudioRequest) -> Call<'_, Opened>;
    fn begin_pcm(&self, start: Duration) -> Result<PcmOpening>;
}

#[derive(Clone)]
pub struct Grant(Arc<dyn AudioAccess>);

impl Grant {
    pub fn new(access: impl AudioAccess + 'static) -> Self {
        Self(Arc::new(access))
    }
}

impl Deref for Grant {
    type Target = dyn AudioAccess;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl From<PlaybackSource> for Grant {
    fn from(source: PlaybackSource) -> Self {
        Self::new(LocalAudio(source))
    }
}

pub trait Sources: Send + Sync {
    fn candidates<'a>(
        &'a self,
        actor: &'a Actor,
        recording: &'a str,
    ) -> Call<'a, Box<dyn Candidates>>;
    fn refresh_until<'a>(
        &'a self,
        actor: &'a Actor,
        recording: &'a str,
        stop: Stop<'a>,
    ) -> Call<'a, bool>;
    fn resolve_until<'a>(
        &'a self,
        actor: &'a Actor,
        recording: &'a str,
        request: ResolveSource,
        stop: Stop<'a>,
    ) -> Call<'a, SourceResolution>;
    fn sources<'a>(&'a self, actor: &'a Actor, recording: &'a str) -> Call<'a, Vec<Source>>;
    fn prepare<'a>(
        &'a self,
        actor: &'a Actor,
        recording: &'a str,
        source: &'a str,
        request: PrepareSource,
    ) -> Call<'a, SourcePreparationReceipt>;
    fn preparation<'a>(
        &'a self,
        actor: &'a Actor,
        recording: &'a str,
        source: &'a str,
    ) -> Call<'a, SourcePreparationStatus>;
}

#[derive(Clone)]
pub struct Seam {
    pub authorization: Arc<dyn Authorization>,
    pub catalog: Arc<dyn CatalogReads>,
    pub progress: Arc<dyn Progress>,
    pub sources: Arc<dyn Sources>,
}

impl Seam {
    pub fn in_process(services: &Services) -> Self {
        let local = Arc::new(InProcess(services.clone()));
        Self {
            authorization: local.clone(),
            catalog: local.clone(),
            progress: local.clone(),
            sources: local,
        }
    }
}

struct InProcess(Services);

impl Authorization for InProcess {
    fn check_session<'a>(&'a self, actor: &'a Actor) -> Call<'a, ()> {
        Box::pin(self.0.identity.check_session(actor))
    }

    fn check_audiobook_chapters<'a>(
        &'a self,
        actor: &'a Actor,
        chapters: &'a [AudiobookChapterAnchor],
    ) -> Call<'a, ()> {
        Box::pin(self.0.collection.check_audiobook_chapters(actor, chapters))
    }

    fn admit_audiobook<'a>(
        &'a self,
        actor: &'a Actor,
        target: AudiobookChapterAnchor,
        chapters: &'a [AudiobookChapterAnchor],
    ) -> Call<'a, Box<dyn Admission>> {
        Box::pin(async move {
            let admission = self
                .0
                .collection
                .admit_audiobook_playback(actor, target, chapters)
                .await?;
            Ok(Box::new(LocalAdmission(admission)) as Box<dyn Admission>)
        })
    }
}

struct LocalAdmission(AudiobookPlaybackAdmission);

impl Admission for LocalAdmission {
    fn publish(self: Box<Self>, publish: Box<dyn FnOnce() + '_>) -> Result<()> {
        self.0.publish(publish)
    }
}

impl CatalogReads for InProcess {
    fn recordings<'a>(&'a self, actor: &'a Actor, ids: &'a [Uuid]) -> Call<'a, Vec<Recording>> {
        Box::pin(self.0.catalog.recordings_by_ids(actor, ids))
    }

    fn release_artwork<'a>(
        &'a self,
        actor: &'a Actor,
        release: &'a str,
    ) -> Call<'a, Option<Artwork>> {
        Box::pin(self.0.catalog.release_artwork(actor, release))
    }

    fn artwork<'a>(&'a self, actor: &'a Actor, recording: &'a str) -> Call<'a, Option<Artwork>> {
        Box::pin(self.0.catalog.artwork(actor, recording))
    }

    fn audiobook_playback<'a>(
        &'a self,
        actor: &'a Actor,
        audiobook: &'a str,
    ) -> Call<'a, AudiobookPlayback> {
        Box::pin(self.0.collection.audiobook_playback(actor, audiobook))
    }
}

impl Progress for InProcess {
    fn saved_position<'a>(
        &'a self,
        actor: &'a Actor,
        recording: Uuid,
    ) -> Call<'a, Option<SavedPosition>> {
        Box::pin(self.0.collection.saved_position(actor, recording))
    }

    fn apply<'a>(&'a self, actor: &'a Actor, batch: SyncBatch) -> Call<'a, SyncReceipt> {
        Box::pin(self.0.collection.apply(actor, batch))
    }
}

struct LocalCandidates(PlaybackCandidates);

impl Candidates for LocalCandidates {
    fn ended(&self) -> Option<CandidatesEnd> {
        self.0.ended()
    }

    fn next(&mut self) -> Call<'_, Option<Grant>> {
        Box::pin(async move { Ok(self.0.next().await?.map(Grant::from)) })
    }
}

impl Sources for InProcess {
    fn candidates<'a>(
        &'a self,
        actor: &'a Actor,
        recording: &'a str,
    ) -> Call<'a, Box<dyn Candidates>> {
        Box::pin(async move {
            let candidates = self
                .0
                .playback
                .playback_candidates(actor, recording)
                .await?;
            Ok(Box::new(LocalCandidates(candidates)) as Box<dyn Candidates>)
        })
    }

    fn refresh_until<'a>(
        &'a self,
        actor: &'a Actor,
        recording: &'a str,
        stop: Stop<'a>,
    ) -> Call<'a, bool> {
        Box::pin(
            self.0
                .catalog
                .refresh_playback_sources_until(actor, recording, stop),
        )
    }

    fn resolve_until<'a>(
        &'a self,
        actor: &'a Actor,
        recording: &'a str,
        request: ResolveSource,
        stop: Stop<'a>,
    ) -> Call<'a, SourceResolution> {
        Box::pin(
            self.0
                .catalog
                .resolve_source_until(actor, recording, request, stop),
        )
    }

    fn sources<'a>(&'a self, actor: &'a Actor, recording: &'a str) -> Call<'a, Vec<Source>> {
        Box::pin(self.0.catalog.sources(actor, recording))
    }

    fn prepare<'a>(
        &'a self,
        actor: &'a Actor,
        recording: &'a str,
        source: &'a str,
        request: PrepareSource,
    ) -> Call<'a, SourcePreparationReceipt> {
        Box::pin(
            self.0
                .catalog
                .prepare_source(actor, recording, source, request),
        )
    }

    fn preparation<'a>(
        &'a self,
        actor: &'a Actor,
        recording: &'a str,
        source: &'a str,
    ) -> Call<'a, SourcePreparationStatus> {
        Box::pin(self.0.catalog.source_preparation(actor, recording, source))
    }
}

struct LocalAudio(PlaybackSource);

struct LocalChunks(hito_core::AudioStream);

impl Chunks for LocalChunks {
    fn chunk(&mut self) -> Call<'_, Option<Bytes>> {
        Box::pin(self.0.chunk())
    }
}

impl AudioAccess for LocalAudio {
    fn source_id(&self) -> &str {
        self.0.source_id()
    }

    fn is_local(&self) -> bool {
        self.0.is_local()
    }

    fn check(&self) -> Call<'_, ()> {
        Box::pin(self.0.check())
    }

    fn revoked(&self) -> Pin<Box<dyn Future<Output = Error> + Send + '_>> {
        Box::pin(self.0.revoked())
    }

    fn open_file(&self) -> Call<'_, AudioFile> {
        Box::pin(self.0.open_file())
    }

    fn open(&self, request: AudioRequest) -> Call<'_, Opened> {
        Box::pin(async move {
            Ok(match self.0.open(request).await? {
                OpenedAudio::Content(mut audio) => Opened::Content(Content {
                    total_length: audio.total_length,
                    range: audio.range.clone(),
                    seekable: audio.seekable,
                    body: Box::new(LocalChunks(
                        audio
                            .body
                            .take()
                            .ok_or(Error::Upstream("Audio did not return content"))?,
                    )),
                }),
                OpenedAudio::Unsatisfiable { length } => Opened::Unsatisfiable { length },
            })
        })
    }

    fn begin_pcm(&self, start: Duration) -> Result<PcmOpening> {
        self.0.begin_pcm(start)
    }
}
