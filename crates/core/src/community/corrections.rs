use super::{
    Community, CommunityReportAdmission, SameRecording,
    source::{self, Provider, PublicSource},
};
use crate::{Actor, Error, Result};
use sqlx::SqliteConnection;
use std::time::{Duration, Instant};
use uuid::Uuid;

const ACCOUNT_PENDING: i64 = 16;
const GLOBAL_PENDING: i64 = 128;

#[derive(sqlx::FromRow)]
pub(super) struct Correction {
    pub(super) user_id: String,
    pub(super) submission_id: String,
    pub(super) session_hash: String,
    left_source_id: String,
    right_source_id: String,
    left_provider: String,
    left_recording_id: String,
    right_provider: String,
    right_recording_id: String,
    pub(super) attempts: i64,
    pub(super) lease_token: String,
}

fn provider(stored: &str) -> Result<Provider> {
    match stored {
        "youtube" => Ok(Provider::Youtube),
        "jamendo" => Ok(Provider::Jamendo),
        _ => Err(Error::Internal(
            "Stored community correction is invalid".into(),
        )),
    }
}

impl Correction {
    pub(super) fn sources(&self) -> Result<(PublicSource, PublicSource)> {
        Ok((
            PublicSource {
                provider: provider(&self.left_provider)?,
                recording_id: self.left_recording_id.clone(),
            },
            PublicSource {
                provider: provider(&self.right_provider)?,
                recording_id: self.right_recording_id.clone(),
            },
        ))
    }
}

async fn recording_of(connection: &mut SqliteConnection, source: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT recording_id FROM sources WHERE id=?")
            .bind(source)
            .fetch_optional(connection)
            .await?,
    )
}

impl Community {
    #[tracing::instrument(level = "debug", skip_all, fields(request_id = %Uuid::new_v4()))]
    pub async fn suggest_same_recording(
        &self,
        actor: &Actor,
        source: &str,
        request: SameRecording,
    ) -> Result<CommunityReportAdmission> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            self.admit_same_recording(actor, source, request),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community correction admission exceeded its time limit",
        )));
        tracing::debug!(
            outcome = result.as_ref().map_or_else(
                |error| error.code(),
                |admission| match admission {
                    CommunityReportAdmission::Ignored => "ignored",
                    CommunityReportAdmission::Queued => "queued",
                    CommunityReportAdmission::AlreadyAccepted => "already_accepted",
                }
            ),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Community same-recording suggestion admitted"
        );
        result
    }

    async fn admit_same_recording(
        &self,
        actor: &Actor,
        left: &str,
        request: SameRecording,
    ) -> Result<CommunityReportAdmission> {
        actor.check_session(&self.store).await?;
        if request.submission_id.get_version_num() != 4 {
            return Err(Error::Invalid(
                "Use a version-4 community submission identifier".into(),
            ));
        }
        let right = request.other_source_id.as_str();
        if left == right {
            return Err(Error::Invalid(
                "Choose two different sources of the same recording".into(),
            ));
        }
        let Some(remote) = &self.remote else {
            return Ok(CommunityReportAdmission::Ignored);
        };
        if !self.vault.ready() {
            return Ok(CommunityReportAdmission::Ignored);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if !super::read(&mut tx, actor).await?.metadata_corrections {
            return Ok(CommunityReportAdmission::Ignored);
        }
        let Some(left_public) = source::resolve(&mut tx, actor, &self.host, left).await? else {
            return Err(Error::NotFound);
        };
        let Some(right_public) = source::resolve(&mut tx, actor, &self.host, right).await? else {
            return Err(Error::NotFound);
        };
        if recording_of(&mut tx, left).await? != recording_of(&mut tx, right).await? {
            return Err(Error::Invalid(
                "Both sources must belong to the same recording".into(),
            ));
        }
        maintain(&mut tx, &remote.config.service_key).await?;
        let previous: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT service_key,left_source_id,right_source_id,status FROM community_corrections WHERE user_id=? AND submission_id=?",
        )
        .bind(actor.id())
        .bind(request.submission_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((service, previous_left, previous_right, status)) = previous {
            if service != remote.config.service_key
                || previous_left != left
                || previous_right != right
            {
                return Err(Error::Conflict(
                    "This community submission belongs to other sources".into(),
                ));
            }
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            return Ok(match status.as_str() {
                "accepted" => CommunityReportAdmission::AlreadyAccepted,
                "queued" | "running" => CommunityReportAdmission::Queued,
                _ => CommunityReportAdmission::Ignored,
            });
        }
        let (account_pending, global_pending): (i64, i64) = sqlx::query_as(
            "SELECT count(*) FILTER(WHERE user_id=?),count(*) FROM community_corrections WHERE status IN ('queued','running')",
        )
        .bind(actor.id())
        .fetch_one(&mut *tx)
        .await?;
        if account_pending >= ACCOUNT_PENDING || global_pending >= GLOBAL_PENDING {
            return Ok(CommunityReportAdmission::Ignored);
        }
        sqlx::query("INSERT INTO community_corrections(user_id,submission_id,service_key,session_hash,left_source_id,right_source_id,left_provider,left_recording_id,right_provider,right_recording_id,status) VALUES(?,?,?,?,?,?,?,?,?,?,'queued')")
            .bind(actor.id())
            .bind(request.submission_id.to_string())
            .bind(&remote.config.service_key)
            .bind(actor.session_binding())
            .bind(left)
            .bind(right)
            .bind(left_public.provider.as_str())
            .bind(left_public.recording_id)
            .bind(right_public.provider.as_str())
            .bind(right_public.recording_id)
            .execute(&mut *tx)
            .await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        Ok(CommunityReportAdmission::Queued)
    }

    pub(super) async fn claim_correction(&self, service: &str) -> Result<Option<Correction>> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        maintain(&mut tx, service).await?;
        let candidate: Option<(String, String)> = sqlx::query_as(
            "SELECT candidate.user_id,candidate.submission_id FROM community_corrections candidate
             WHERE candidate.service_key=? AND candidate.status='queued' AND candidate.attempts<3 AND candidate.next_attempt_at<=unixepoch()
                AND NOT EXISTS(SELECT 1 FROM community_corrections running WHERE running.user_id=candidate.user_id AND running.status='running')
                AND (SELECT count(*) FROM community_corrections WHERE status='running')<2
             ORDER BY candidate.created_at,candidate.submission_id LIMIT 1",
        )
        .bind(service)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((account, submission)) = candidate else {
            tx.commit().await?;
            return Ok(None);
        };
        let token = Uuid::new_v4().to_string();
        sqlx::query("UPDATE community_corrections SET status='running',attempts=attempts+1,lease_token=?,lease_until=unixepoch()+10,updated_at=unixepoch() WHERE user_id=? AND submission_id=? AND status='queued'")
            .bind(token).bind(&account).bind(&submission).execute(&mut *tx).await?;
        let correction = sqlx::query_as("SELECT user_id,submission_id,session_hash,left_source_id,right_source_id,left_provider,left_recording_id,right_provider,right_recording_id,attempts,lease_token FROM community_corrections WHERE user_id=? AND submission_id=?")
            .bind(account).bind(submission).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(correction))
    }

    pub(super) async fn check_correction_on(
        &self,
        connection: &mut SqliteConnection,
        correction: &Correction,
    ) -> Result<Actor> {
        let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM community_corrections WHERE user_id=? AND submission_id=? AND lease_token=? AND status='running' AND lease_until>unixepoch() AND created_at>unixepoch()-604800)")
            .bind(&correction.user_id).bind(&correction.submission_id).bind(&correction.lease_token).fetch_one(&mut *connection).await?;
        if !current {
            return Err(Error::NotFound);
        }
        let actor =
            Actor::resume_session_on(connection, &correction.user_id, &correction.session_hash)
                .await?;
        let (left, right) = correction.sources()?;
        if !super::read(connection, &actor).await?.metadata_corrections
            || source::resolve(connection, &actor, &self.host, &correction.left_source_id)
                .await?
                .as_ref()
                != Some(&left)
            || source::resolve(connection, &actor, &self.host, &correction.right_source_id)
                .await?
                .as_ref()
                != Some(&right)
        {
            return Err(Error::NotFound);
        }
        Ok(actor)
    }

    pub(super) async fn check_correction(&self, correction: &Correction) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(1), async {
            let mut tx = self.store.pool.begin().await?;
            let actor = self.check_correction_on(&mut tx, correction).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community authority could not be checked in time",
        )))
    }

    pub(super) async fn finish_correction(
        &self,
        correction: &Correction,
        result: Result<()>,
        stopped: bool,
    ) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = match self.check_correction_on(&mut tx, correction).await {
            Ok(_) => true,
            Err(Error::NotFound | Error::Unauthorized) => false,
            Err(error) => return Err(error),
        };
        let status = if !current {
            "cancelled"
        } else if stopped {
            "queued"
        } else if result.is_ok() {
            "accepted"
        } else if correction.attempts < 3 {
            "queued"
        } else {
            "failed"
        };
        sqlx::query("UPDATE community_corrections SET status=?,attempts=attempts-?,lease_token=NULL,lease_until=NULL,next_attempt_at=unixepoch()+?,updated_at=unixepoch() WHERE user_id=? AND submission_id=? AND lease_token=? AND status='running'")
            .bind(status).bind(i64::from(stopped)).bind(if stopped { 0 } else { 1i64 << correction.attempts })
            .bind(&correction.user_id).bind(&correction.submission_id).bind(&correction.lease_token).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(status)
    }
}

async fn maintain(connection: &mut SqliteConnection, service: &str) -> Result<()> {
    sqlx::query("UPDATE community_corrections SET status='cancelled',lease_token=NULL,lease_until=NULL,updated_at=unixepoch()
        WHERE status IN ('queued','running') AND (service_key!=? OR created_at<=unixepoch()-604800
            OR NOT EXISTS(SELECT 1 FROM sessions WHERE token_hash=community_corrections.session_hash AND user_id=community_corrections.user_id AND expires_at>unixepoch())
            OR NOT EXISTS(SELECT 1 FROM account_community_preferences WHERE user_id=community_corrections.user_id AND metadata_corrections=1))")
        .bind(service).execute(&mut *connection).await?;
    sqlx::query("UPDATE community_corrections SET status=CASE WHEN attempts<3 THEN 'queued' ELSE 'failed' END,lease_token=NULL,lease_until=NULL,next_attempt_at=unixepoch()+2,updated_at=unixepoch() WHERE status='running' AND lease_until<=unixepoch()")
        .execute(&mut *connection).await?;
    sqlx::query("DELETE FROM community_corrections WHERE status NOT IN ('queued','running') AND updated_at<unixepoch()-604800")
        .execute(&mut *connection).await?;
    sqlx::query("DELETE FROM community_corrections WHERE (user_id,submission_id) IN (SELECT user_id,submission_id FROM community_corrections WHERE status NOT IN ('queued','running') ORDER BY updated_at DESC,submission_id DESC LIMIT -1 OFFSET 1024)")
        .execute(connection).await?;
    Ok(())
}
