use crate::{
    Error, TorBoxClient, TorrentFileBinding, TorrentPhase,
    transport::{CandidateOrigin, Transport},
    workflow::WorkflowLog,
};
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::sync::OwnedSemaphorePermit;
use tracing::Instrument;
use zeroize::{Zeroize, Zeroizing};

pub struct GeneratedFileCandidate {
    binding: TorrentFileBinding,
    private_link: Zeroizing<String>,
    origin: CandidateOrigin,
}

pub struct VerifiedFilePreparation<'a> {
    client: &'a TorBoxClient,
    binding: TorrentFileBinding,
    permit: OwnedSemaphorePermit,
    deadline: tokio::time::Instant,
    log: WorkflowLog,
}

impl GeneratedFileCandidate {
    pub(crate) fn new(
        binding: &TorrentFileBinding,
        private_link: Zeroizing<String>,
        origin: CandidateOrigin,
    ) -> Result<Self, Error> {
        if private_link.is_empty() || private_link.len() > 8192 {
            return Err(Error::InvalidResponse);
        }
        Ok(Self {
            binding: binding.clone(),
            private_link,
            origin,
        })
    }

    pub fn binding(&self) -> &TorrentFileBinding {
        &self.binding
    }
    pub(crate) fn private_link(&self) -> &str {
        &self.private_link
    }

    pub(crate) fn belongs_to(&self, transport: &Transport) -> bool {
        transport.owns_candidate(&self.origin)
    }
}

#[derive(Deserialize)]
struct Acknowledgment {
    success: bool,
    error: Option<serde::de::IgnoredAny>,
    data: Option<String>,
}

impl Drop for Acknowledgment {
    fn drop(&mut self) {
        if let Some(link) = &mut self.data {
            link.zeroize();
        }
    }
}

impl TorBoxClient {
    pub async fn prepare_bound_file(
        &self,
        binding: &TorrentFileBinding,
    ) -> Result<GeneratedFileCandidate, Error> {
        self.verify_bound_file(binding).await?.mint().await
    }

    pub async fn verify_bound_file(
        &self,
        binding: &TorrentFileBinding,
    ) -> Result<VerifiedFilePreparation<'_>, Error> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
        let mut log = WorkflowLog {
            started: Instant::now(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            span: tracing::debug_span!("torbox_preparation", request_id = %uuid::Uuid::new_v4(), operation = "bound_file_preparation"),
            event: "torbox_preparation_finished",
            outcome: "cancelled",
            count: 0,
            mint_dispatched: false,
        };
        let result = tokio::time::timeout_at(
            deadline,
            async {
                let current = self.observe(binding).await?;
                if current.phase != TorrentPhase::ProviderCompleted {
                    return Err(Error::SourceIncomplete);
                }
                self.transport.admit()
            }
            .instrument(log.span.clone()),
        )
        .await
        .unwrap_or(Err(Error::Deadline));
        match result {
            Ok(permit) => Ok(VerifiedFilePreparation {
                client: self,
                binding: binding.clone(),
                permit,
                deadline,
                log,
            }),
            Err(error) => {
                log.outcome = error.outcome();
                Err(error)
            }
        }
    }
}

impl VerifiedFilePreparation<'_> {
    pub async fn mint(self) -> Result<GeneratedFileCandidate, Error> {
        let Self {
            client,
            binding,
            permit: _permit,
            deadline,
            mut log,
        } = self;
        let result = if tokio::time::Instant::now() >= deadline {
            Err(Error::Deadline)
        } else {
            tokio::time::timeout_at(
                deadline,
                async {
                    let bytes = client
                        .transport
                        .mint_file(
                            binding.torrent_id,
                            binding.file_id,
                            deadline,
                            &mut log.mint_dispatched,
                        )
                        .await?;
                    candidate(&bytes, &binding, client.transport.candidate_origin())
                }
                .instrument(log.span.clone()),
            )
            .await
            .unwrap_or(Err(Error::Deadline))
        };
        let result = if log.mint_dispatched {
            result.map_err(|error| match error {
                Error::Status(401 | 403) | Error::ProviderRejected => error,
                _ => Error::LinkUnknown,
            })
        } else {
            result
        };
        log.outcome = result
            .as_ref()
            .map(|_| "candidate")
            .unwrap_or_else(|error| error.outcome());
        log.count = u64::from(result.is_ok());
        result
    }
}

fn candidate(
    bytes: &[u8],
    binding: &TorrentFileBinding,
    origin: CandidateOrigin,
) -> Result<GeneratedFileCandidate, Error> {
    let mut ack: Acknowledgment =
        serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    if !ack.success || ack.error.is_some() {
        return Err(Error::ProviderRejected);
    }
    GeneratedFileCandidate::new(
        binding,
        Zeroizing::new(ack.data.take().ok_or(Error::InvalidResponse)?),
        origin,
    )
}
