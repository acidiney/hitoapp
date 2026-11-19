use hito_core::{
    Actor, Artwork, AudiobookChapterAnchor, AudiobookPlayback, CandidatesEnd, PlaybackCandidates,
    PlaybackSource, PrepareSource, Recording, ResolveSource, Result, SavedPosition, Services,
    Source, SourcePreparationReceipt, SourcePreparationStatus, SourceResolution, SyncBatch,
    SyncReceipt,
};
use std::{future::Future, pin::Pin, sync::Arc};
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
    fn next(&mut self) -> Call<'_, Option<PlaybackSource>>;
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

    fn next(&mut self) -> Call<'_, Option<PlaybackSource>> {
        Box::pin(self.0.next())
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
