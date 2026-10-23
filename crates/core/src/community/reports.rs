use super::{
    Community, CommunityReportAdmission, SuccessfulPlayback,
    source::{self, Provider, PublicSource},
};
use crate::{Actor, Error, Result};
use sqlx::SqliteConnection;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(super) struct Report {
    pub(super) user_id: String,
    pub(super) submission_id: String,
    pub(super) session_hash: String,
    pub(super) source_id: String,
    provider: String,
    public_recording_id: String,
    pub(super) attempts: i64,
    pub(super) lease_token: String,
}

impl Report {
    pub(super) fn source(&self) -> Result<PublicSource> {
        let provider = match self.provider.as_str() {
            "youtube" => Provider::Youtube,
            "jamendo" => Provider::Jamendo,
            _ => return Err(Error::Internal("Stored community source is invalid".into())),
        };
        Ok(PublicSource {
            provider,
            recording_id: self.public_recording_id.clone(),
        })
    }
}

impl Community {
    #[tracing::instrument(level = "debug", skip_all, fields(request_id = %Uuid::new_v4()))]
    pub async fn observe_success(
        &self,
        actor: &Actor,
        source: &str,
        request: SuccessfulPlayback,
    ) -> Result<CommunityReportAdmission> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            self.admit_success(actor, source, request),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community report admission exceeded its time limit",
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
            "Community playback claim admitted"
        );
        result
    }

    async fn admit_success(
        &self,
        actor: &Actor,
        source: &str,
        request: SuccessfulPlayback,
    ) -> Result<CommunityReportAdmission> {
        actor.check_session(&self.store).await?;
        if request.submission_id.get_version_num() != 4 {
            return Err(Error::Invalid(
                "Use a version-4 community submission identifier".into(),
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
        if !super::read(&mut tx, actor).await?.playback_reporting {
            return Ok(CommunityReportAdmission::Ignored);
        }
        let Some(public) = source::resolve(&mut tx, actor, &self.host, source).await? else {
            return Ok(CommunityReportAdmission::Ignored);
        };
        maintain(&mut tx, &remote.config.service_key).await?;
        let previous: Option<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT service_key,source_id,provider,public_recording_id,status FROM community_reports WHERE user_id=? AND submission_id=?",
        ).bind(actor.id()).bind(request.submission_id.to_string()).fetch_optional(&mut *tx).await?;
        if let Some((service, previous_source, provider, recording, status)) = previous {
            if service != remote.config.service_key
                || previous_source != source
                || provider != public.provider.as_str()
                || recording != public.recording_id
            {
                return Err(Error::Conflict(
                    "This community submission belongs to another source".into(),
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
            "SELECT count(*) FILTER(WHERE user_id=?),count(*) FROM community_reports WHERE status IN ('queued','running')",
        ).bind(actor.id()).fetch_one(&mut *tx).await?;
        if account_pending >= 64 || global_pending >= 256 {
            return Ok(CommunityReportAdmission::Ignored);
        }
        sqlx::query("INSERT INTO community_reports(user_id,submission_id,service_key,session_hash,source_id,provider,public_recording_id,status) VALUES(?,?,?,?,?,?,?,'queued')")
            .bind(actor.id()).bind(request.submission_id.to_string()).bind(&remote.config.service_key)
            .bind(actor.session_binding()).bind(source).bind(public.provider.as_str()).bind(public.recording_id)
            .execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        Ok(CommunityReportAdmission::Queued)
    }

    pub(super) async fn claim_report(&self, service: &str) -> Result<Option<Report>> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        maintain(&mut tx, service).await?;
        let candidate: Option<(String, String)> = sqlx::query_as(
            "SELECT candidate.user_id,candidate.submission_id FROM community_reports candidate
             WHERE candidate.service_key=? AND candidate.status='queued' AND candidate.attempts<3 AND candidate.next_attempt_at<=unixepoch()
                AND NOT EXISTS(SELECT 1 FROM community_reports running WHERE running.user_id=candidate.user_id AND running.status='running')
                AND (SELECT count(*) FROM community_reports WHERE status='running')<2
             ORDER BY candidate.created_at,candidate.submission_id LIMIT 1",
        ).bind(service).fetch_optional(&mut *tx).await?;
        let Some((account, submission)) = candidate else {
            tx.commit().await?;
            return Ok(None);
        };
        let token = Uuid::new_v4().to_string();
        sqlx::query("UPDATE community_reports SET status='running',attempts=attempts+1,lease_token=?,lease_until=unixepoch()+10,updated_at=unixepoch() WHERE user_id=? AND submission_id=? AND status='queued'")
            .bind(token).bind(&account).bind(&submission).execute(&mut *tx).await?;
        let report = sqlx::query_as("SELECT user_id,submission_id,session_hash,source_id,provider,public_recording_id,attempts,lease_token FROM community_reports WHERE user_id=? AND submission_id=?")
            .bind(account).bind(submission).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(report))
    }

    pub(super) async fn check_report_on(
        &self,
        connection: &mut SqliteConnection,
        report: &Report,
    ) -> Result<Actor> {
        let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM community_reports WHERE user_id=? AND submission_id=? AND lease_token=? AND status='running' AND lease_until>unixepoch() AND created_at>unixepoch()-604800)")
            .bind(&report.user_id).bind(&report.submission_id).bind(&report.lease_token).fetch_one(&mut *connection).await?;
        if !current {
            return Err(Error::NotFound);
        }
        let actor =
            Actor::resume_session_on(connection, &report.user_id, &report.session_hash).await?;
        if !super::read(connection, &actor).await?.playback_reporting
            || source::resolve(connection, &actor, &self.host, &report.source_id)
                .await?
                .as_ref()
                != Some(&report.source()?)
        {
            return Err(Error::NotFound);
        }
        Ok(actor)
    }

    pub(super) async fn check_report(&self, report: &Report) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(1), async {
            let mut tx = self.store.pool.begin().await?;
            let actor = self.check_report_on(&mut tx, report).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community authority could not be checked in time",
        )))
    }

    pub(super) async fn finish_report(
        &self,
        report: &Report,
        result: Result<()>,
        stopped: bool,
    ) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = match self.check_report_on(&mut tx, report).await {
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
        } else if report.attempts < 3 {
            "queued"
        } else {
            "failed"
        };
        sqlx::query("UPDATE community_reports SET status=?,attempts=attempts-?,lease_token=NULL,lease_until=NULL,next_attempt_at=unixepoch()+?,updated_at=unixepoch() WHERE user_id=? AND submission_id=? AND lease_token=? AND status='running'")
            .bind(status).bind(i64::from(stopped)).bind(if stopped { 0 } else { 1i64 << report.attempts })
            .bind(&report.user_id).bind(&report.submission_id).bind(&report.lease_token).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(status)
    }
}

async fn maintain(connection: &mut SqliteConnection, service: &str) -> Result<()> {
    sqlx::query("UPDATE community_reports SET status='cancelled',lease_token=NULL,lease_until=NULL,updated_at=unixepoch()
        WHERE status IN ('queued','running') AND (service_key!=? OR created_at<=unixepoch()-604800
            OR NOT EXISTS(SELECT 1 FROM sessions WHERE token_hash=community_reports.session_hash AND user_id=community_reports.user_id AND expires_at>unixepoch())
            OR NOT EXISTS(SELECT 1 FROM account_community_preferences WHERE user_id=community_reports.user_id AND playback_reporting=1))")
        .bind(service).execute(&mut *connection).await?;
    sqlx::query("UPDATE community_reports SET status=CASE WHEN attempts<3 THEN 'queued' ELSE 'failed' END,lease_token=NULL,lease_until=NULL,next_attempt_at=unixepoch()+2,updated_at=unixepoch() WHERE status='running' AND lease_until<=unixepoch()")
        .execute(&mut *connection).await?;
    sqlx::query("DELETE FROM community_reports WHERE status NOT IN ('queued','running') AND (updated_at<unixepoch()-604800 OR (user_id,submission_id) IN (
        SELECT user_id,submission_id FROM (SELECT user_id,submission_id,row_number() OVER(PARTITION BY user_id ORDER BY updated_at DESC,submission_id DESC) AS ordinal FROM community_reports WHERE status NOT IN ('queued','running')) WHERE ordinal>1024))")
        .execute(&mut *connection).await?;
    sqlx::query("DELETE FROM community_reports WHERE (user_id,submission_id) IN (SELECT user_id,submission_id FROM community_reports WHERE status NOT IN ('queued','running') ORDER BY updated_at DESC,submission_id DESC LIMIT -1 OFFSET 4096)")
        .execute(connection).await?;
    Ok(())
}
