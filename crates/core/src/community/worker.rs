use super::{Community, config, corrections::Correction, reports::Report, transport::Transport};
use crate::{Error, Result};
use sqlx::SqliteConnection;
use std::{
    future::Future,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::Zeroizing;

impl Community {
    #[tracing::instrument(level = "debug", skip_all, fields(request_id = %Uuid::new_v4()))]
    pub async fn run_next_report_until(
        &self,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<bool> {
        let Some(remote) = &self.remote else {
            return Ok(false);
        };
        if !self.vault.ready() {
            return Ok(false);
        }
        let report = tokio::time::timeout(
            Duration::from_secs(1),
            self.claim_report(&remote.config.service_key),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community report claiming exceeded its time limit",
        )))?;
        let Some(report) = report else {
            return self.run_next_correction_until(remote, shutdown).await;
        };
        let started = Instant::now();
        let mut changed = self.changed.subscribe();
        let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let (result, stopped) = {
            let delivery =
                tokio::time::timeout(Duration::from_secs(8), self.deliver_report(remote, &report));
            tokio::pin!(delivery);
            tokio::pin!(shutdown);
            loop {
                tokio::select! {
                biased;
                _ = &mut shutdown => break (Err(Error::Upstream("Community delivery stopped")), true),
                _ = changed.changed() => {
                    if let Err(error) = self.check_report(&report).await { break (Err(error), false); }
                },
                _ = heartbeat.tick() => {
                    if let Err(error) = self.check_report(&report).await { break (Err(error), false); }
                },
                result = &mut delivery => break (result.unwrap_or(Err(Error::Upstream("Community delivery exceeded its time limit"))), false),
                }
            }
        };
        let status = tokio::time::timeout(
            Duration::from_secs(1),
            self.finish_report(&report, result, stopped),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community delivery could not be acknowledged in time",
        )))?;
        tracing::debug!(submission_id = %report.submission_id, outcome = status,
            elapsed_ms = started.elapsed().as_millis() as u64, "Community delivery finished");
        Ok(true)
    }

    async fn run_next_correction_until(
        &self,
        remote: &Transport,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<bool> {
        let correction = tokio::time::timeout(
            Duration::from_secs(1),
            self.claim_correction(&remote.config.service_key),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community correction claiming exceeded its time limit",
        )))?;
        let Some(correction) = correction else {
            return Ok(false);
        };
        let started = Instant::now();
        let mut changed = self.changed.subscribe();
        let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let (result, stopped) = {
            let delivery = tokio::time::timeout(
                Duration::from_secs(8),
                self.deliver_correction(remote, &correction),
            );
            tokio::pin!(delivery);
            tokio::pin!(shutdown);
            loop {
                tokio::select! {
                biased;
                _ = &mut shutdown => break (Err(Error::Upstream("Community delivery stopped")), true),
                _ = changed.changed() => {
                    if let Err(error) = self.check_correction(&correction).await { break (Err(error), false); }
                },
                _ = heartbeat.tick() => {
                    if let Err(error) = self.check_correction(&correction).await { break (Err(error), false); }
                },
                result = &mut delivery => break (result.unwrap_or(Err(Error::Upstream("Community delivery exceeded its time limit"))), false),
                }
            }
        };
        let status = tokio::time::timeout(
            Duration::from_secs(1),
            self.finish_correction(&correction, result, stopped),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community delivery could not be acknowledged in time",
        )))?;
        tracing::debug!(submission_id = %correction.submission_id, outcome = status,
            elapsed_ms = started.elapsed().as_millis() as u64, "Community correction delivery finished");
        Ok(true)
    }

    async fn deliver_correction(&self, remote: &Transport, correction: &Correction) -> Result<()> {
        self.check_correction(correction).await?;
        let token = self.reporter_token(remote, correction).await?;
        self.check_correction(correction).await?;
        let submission = Uuid::parse_str(&correction.submission_id)
            .map_err(|_| Error::Internal("Stored community submission is invalid".into()))?;
        let (left, right) = correction.sources()?;
        remote.suggest(&token, submission, &left, &right).await?;
        self.check_correction(correction).await
    }

    async fn deliver_report(&self, remote: &Transport, report: &Report) -> Result<()> {
        self.check_report(report).await?;
        let token = self.reporter_token(remote, report).await?;
        self.check_report(report).await?;
        let submission = Uuid::parse_str(&report.submission_id)
            .map_err(|_| Error::Internal("Stored community submission is invalid".into()))?;
        remote.submit(&token, submission, &report.source()?).await?;
        self.check_report(report).await
    }

    async fn reporter_token(
        &self,
        remote: &Transport,
        claimed: &impl Claimed,
    ) -> Result<Zeroizing<String>> {
        let user_id = claimed.user_id();
        let retained: Option<(String, Vec<u8>)> = sqlx::query_as("SELECT reporter_id,credential FROM community_reporters WHERE user_id=? AND service_key=?")
            .bind(user_id).bind(&remote.config.service_key).fetch_optional(&self.store.pool).await?;
        if let Some((reporter, credential)) = retained {
            let token = self.vault.reveal_community(
                user_id,
                &remote.config.service_key,
                &reporter,
                &credential,
            )?;
            if !config::valid_token(&token) {
                return Err(Error::Upstream(
                    "Community reporter credentials are unavailable",
                ));
            }
            return Ok(token);
        }
        claimed.check(self).await?;
        let path = remote.config.admin_token_file.clone();
        let admin = tokio::task::spawn_blocking(move || config::read_admin_token(&path))
            .await
            .map_err(|_| {
                Error::Upstream("Community administrator credentials are unavailable")
            })??;
        claimed.check(self).await?;
        let provisioned = remote.provision(&admin).await?;
        claimed.check(self).await?;
        let reporter = provisioned.id.to_string();
        let credential = self.vault.seal_community(
            user_id,
            &remote.config.service_key,
            &reporter,
            &provisioned.token,
        )?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        claimed.check_on(self, &mut tx).await?;
        sqlx::query("INSERT INTO community_reporters(user_id,service_key,reporter_id,credential) VALUES(?,?,?,?)")
            .bind(user_id).bind(&remote.config.service_key).bind(reporter).bind(credential).execute(&mut *tx).await?;
        tx.commit().await?;
        claimed.check(self).await?;
        Ok(provisioned.token)
    }
}

pub(super) trait Claimed {
    fn user_id(&self) -> &str;
    async fn check(&self, community: &Community) -> Result<()>;
    async fn check_on(
        &self,
        community: &Community,
        connection: &mut SqliteConnection,
    ) -> Result<()>;
}

impl Claimed for Report {
    fn user_id(&self) -> &str {
        &self.user_id
    }
    async fn check(&self, community: &Community) -> Result<()> {
        community.check_report(self).await
    }
    async fn check_on(
        &self,
        community: &Community,
        connection: &mut SqliteConnection,
    ) -> Result<()> {
        community
            .check_report_on(connection, self)
            .await
            .map(|_| ())
    }
}

impl Claimed for Correction {
    fn user_id(&self) -> &str {
        &self.user_id
    }
    async fn check(&self, community: &Community) -> Result<()> {
        community.check_correction(self).await
    }
    async fn check_on(
        &self,
        community: &Community,
        connection: &mut SqliteConnection,
    ) -> Result<()> {
        community
            .check_correction_on(connection, self)
            .await
            .map(|_| ())
    }
}
