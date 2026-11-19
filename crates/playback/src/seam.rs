use bytes::Bytes;
use hito_core::{
    Actor, Artwork, AudioFile, AudioRequest, AudiobookChapterAnchor, AudiobookPlayback,
    AudiobookPlaybackAdmission, CandidatesEnd, Error, OpenedAudio, PcmOpening, PlaybackCandidates,
    PlaybackSource, PrepareSource, Recording, ResolveSource, Result, SavedPosition, Services,
    Source, SourcePreparationReceipt, SourcePreparationStatus, SourceResolution, SyncBatch,
    SyncReceipt, User,
};
use std::{future::Future, ops::Deref, ops::RangeInclusive, pin::Pin, sync::Arc, time::Duration};
use uuid::Uuid;

pub type Call<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
pub type Stop<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

pub trait Authorization: Send + Sync {
    fn check_session<'a>(&'a self, actor: &'a Listener) -> Call<'a, ()>;
    fn check_audiobook_chapters<'a>(
        &'a self,
        actor: &'a Listener,
        chapters: &'a [AudiobookChapterAnchor],
    ) -> Call<'a, ()>;
    fn admit_audiobook<'a>(
        &'a self,
        actor: &'a Listener,
        target: AudiobookChapterAnchor,
        chapters: &'a [AudiobookChapterAnchor],
    ) -> Call<'a, Box<dyn Admission>>;
}

pub trait Admission: Send {
    fn publish(self: Box<Self>, publish: Box<dyn FnOnce() + '_>) -> Result<()>;
}

pub trait CatalogReads: Send + Sync {
    fn recordings<'a>(&'a self, actor: &'a Listener, ids: &'a [Uuid]) -> Call<'a, Vec<Recording>>;
    fn release_artwork<'a>(
        &'a self,
        actor: &'a Listener,
        release: &'a str,
    ) -> Call<'a, Option<Artwork>>;
    fn artwork<'a>(&'a self, actor: &'a Listener, recording: &'a str) -> Call<'a, Option<Artwork>>;
    fn audiobook_playback<'a>(
        &'a self,
        actor: &'a Listener,
        audiobook: &'a str,
    ) -> Call<'a, AudiobookPlayback>;
}

pub trait Progress: Send + Sync {
    fn saved_position<'a>(
        &'a self,
        actor: &'a Listener,
        recording: Uuid,
    ) -> Call<'a, Option<SavedPosition>>;
    fn apply<'a>(&'a self, actor: &'a Listener, batch: SyncBatch) -> Call<'a, SyncReceipt>;
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
        actor: &'a Listener,
        recording: &'a str,
    ) -> Call<'a, Box<dyn Candidates>>;
    fn refresh_until<'a>(
        &'a self,
        actor: &'a Listener,
        recording: &'a str,
        stop: Stop<'a>,
    ) -> Call<'a, bool>;
    fn resolve_until<'a>(
        &'a self,
        actor: &'a Listener,
        recording: &'a str,
        request: ResolveSource,
        stop: Stop<'a>,
    ) -> Call<'a, SourceResolution>;
    fn sources<'a>(&'a self, actor: &'a Listener, recording: &'a str) -> Call<'a, Vec<Source>>;
    fn prepare<'a>(
        &'a self,
        actor: &'a Listener,
        recording: &'a str,
        source: &'a str,
        request: PrepareSource,
    ) -> Call<'a, SourcePreparationReceipt>;
    fn preparation<'a>(
        &'a self,
        actor: &'a Listener,
        recording: &'a str,
        source: &'a str,
    ) -> Call<'a, SourcePreparationStatus>;
}

#[derive(Clone)]
pub struct Listener(Identity);

#[derive(Clone)]
enum Identity {
    Local(Actor),
    Remote { user: User, session: Arc<str> },
}

impl Listener {
    pub fn remote(user: User, session: impl Into<Arc<str>>) -> Self {
        Self(Identity::Remote {
            user,
            session: session.into(),
        })
    }

    pub fn user(&self) -> &User {
        match &self.0 {
            Identity::Local(actor) => actor.user(),
            Identity::Remote { user, .. } => user,
        }
    }

    pub fn same_session(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Identity::Local(left), Identity::Local(right)) => left.same_session(right),
            (
                Identity::Remote { user, session },
                Identity::Remote {
                    user: other_user,
                    session: other_session,
                },
            ) => user.id == other_user.id && session == other_session,
            _ => false,
        }
    }

    pub fn local(&self) -> Result<&Actor> {
        match &self.0 {
            Identity::Local(actor) => Ok(actor),
            Identity::Remote { .. } => Err(Error::Unauthorized),
        }
    }
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener")
            .field("user", &self.user().id)
            .finish_non_exhaustive()
    }
}

impl From<Actor> for Listener {
    fn from(actor: Actor) -> Self {
        Self(Identity::Local(actor))
    }
}

impl From<&Actor> for Listener {
    fn from(actor: &Actor) -> Self {
        Self(Identity::Local(actor.clone()))
    }
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
    fn check_session<'a>(&'a self, actor: &'a Listener) -> Call<'a, ()> {
        Box::pin(async move { self.0.identity.check_session(actor.local()?).await })
    }

    fn check_audiobook_chapters<'a>(
        &'a self,
        actor: &'a Listener,
        chapters: &'a [AudiobookChapterAnchor],
    ) -> Call<'a, ()> {
        Box::pin(async move {
            self.0
                .collection
                .check_audiobook_chapters(actor.local()?, chapters)
                .await
        })
    }

    fn admit_audiobook<'a>(
        &'a self,
        actor: &'a Listener,
        target: AudiobookChapterAnchor,
        chapters: &'a [AudiobookChapterAnchor],
    ) -> Call<'a, Box<dyn Admission>> {
        Box::pin(async move {
            let admission = self
                .0
                .collection
                .admit_audiobook_playback(actor.local()?, target, chapters)
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
    fn recordings<'a>(&'a self, actor: &'a Listener, ids: &'a [Uuid]) -> Call<'a, Vec<Recording>> {
        Box::pin(async move { self.0.catalog.recordings_by_ids(actor.local()?, ids).await })
    }

    fn release_artwork<'a>(
        &'a self,
        actor: &'a Listener,
        release: &'a str,
    ) -> Call<'a, Option<Artwork>> {
        Box::pin(async move {
            self.0
                .catalog
                .release_artwork(actor.local()?, release)
                .await
        })
    }

    fn artwork<'a>(&'a self, actor: &'a Listener, recording: &'a str) -> Call<'a, Option<Artwork>> {
        Box::pin(async move { self.0.catalog.artwork(actor.local()?, recording).await })
    }

    fn audiobook_playback<'a>(
        &'a self,
        actor: &'a Listener,
        audiobook: &'a str,
    ) -> Call<'a, AudiobookPlayback> {
        Box::pin(async move {
            self.0
                .collection
                .audiobook_playback(actor.local()?, audiobook)
                .await
        })
    }
}

impl Progress for InProcess {
    fn saved_position<'a>(
        &'a self,
        actor: &'a Listener,
        recording: Uuid,
    ) -> Call<'a, Option<SavedPosition>> {
        Box::pin(async move {
            self.0
                .collection
                .saved_position(actor.local()?, recording)
                .await
        })
    }

    fn apply<'a>(&'a self, actor: &'a Listener, batch: SyncBatch) -> Call<'a, SyncReceipt> {
        Box::pin(async move { self.0.collection.apply(actor.local()?, batch).await })
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
        actor: &'a Listener,
        recording: &'a str,
    ) -> Call<'a, Box<dyn Candidates>> {
        Box::pin(async move {
            let candidates = self
                .0
                .playback
                .playback_candidates(actor.local()?, recording)
                .await?;
            Ok(Box::new(LocalCandidates(candidates)) as Box<dyn Candidates>)
        })
    }

    fn refresh_until<'a>(
        &'a self,
        actor: &'a Listener,
        recording: &'a str,
        stop: Stop<'a>,
    ) -> Call<'a, bool> {
        Box::pin(async move {
            self.0
                .catalog
                .refresh_playback_sources_until(actor.local()?, recording, stop)
                .await
        })
    }

    fn resolve_until<'a>(
        &'a self,
        actor: &'a Listener,
        recording: &'a str,
        request: ResolveSource,
        stop: Stop<'a>,
    ) -> Call<'a, SourceResolution> {
        Box::pin(async move {
            self.0
                .catalog
                .resolve_source_until(actor.local()?, recording, request, stop)
                .await
        })
    }

    fn sources<'a>(&'a self, actor: &'a Listener, recording: &'a str) -> Call<'a, Vec<Source>> {
        Box::pin(async move { self.0.catalog.sources(actor.local()?, recording).await })
    }

    fn prepare<'a>(
        &'a self,
        actor: &'a Listener,
        recording: &'a str,
        source: &'a str,
        request: PrepareSource,
    ) -> Call<'a, SourcePreparationReceipt> {
        Box::pin(async move {
            self.0
                .catalog
                .prepare_source(actor.local()?, recording, source, request)
                .await
        })
    }

    fn preparation<'a>(
        &'a self,
        actor: &'a Listener,
        recording: &'a str,
        source: &'a str,
    ) -> Call<'a, SourcePreparationStatus> {
        Box::pin(async move {
            self.0
                .catalog
                .source_preparation(actor.local()?, recording, source)
                .await
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str) -> User {
        User {
            id: id.into(),
            username: id.into(),
            admin: false,
            display_name: None,
            avatar_version: None,
        }
    }

    #[test]
    fn a_remote_listener_never_yields_a_core_actor() {
        let listener = Listener::remote(user("remote-account"), "session-key");
        assert!(matches!(listener.local(), Err(Error::Unauthorized)));
        assert_eq!(listener.user().id, "remote-account");
    }

    #[test]
    fn remote_listeners_share_a_session_only_with_the_same_account_and_session() {
        let first = Listener::remote(user("remote-account"), "session-key");
        assert!(first.same_session(&Listener::remote(user("remote-account"), "session-key")));
        assert!(!first.same_session(&Listener::remote(user("remote-account"), "other-key")));
        assert!(!first.same_session(&Listener::remote(user("other-account"), "session-key")));
    }

    #[test]
    fn a_listener_never_prints_its_session() {
        let listener = Listener::remote(user("remote-account"), "session-key");
        assert!(!format!("{listener:?}").contains("session-key"));
    }
}
