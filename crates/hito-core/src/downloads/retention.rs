use super::Downloads;
use crate::{Error, Result};
use sqlx::SqliteConnection;
use uuid::Uuid;

pub(super) enum Admission {
    Retained(String),
    Busy,
    WaitingForSource,
}

impl Downloads {
    pub(super) async fn ensure_transfer_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        recording: &str,
        library: &str,
        grant: &str,
    ) -> Result<String> {
        match self
            .admit_transfer_on(connection, account, recording, library, grant)
            .await?
        {
            Admission::Retained(intent) => Ok(intent),
            Admission::Busy => Err(Error::Conflict("Download work is busy. Try again.".into())),
            Admission::WaitingForSource => Err(Error::Conflict(
                "No verified provider audio source is available for this download".into(),
            )),
        }
    }

    pub(super) async fn admit_transfer_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        recording: &str,
        library: &str,
        grant: &str,
    ) -> Result<Admission> {
        if let Some(intent) = self
            .retain_existing_on(connection, account, recording, grant)
            .await?
        {
            return Ok(Admission::Retained(intent));
        }
        let (account_pending, pending): (i64, i64) = sqlx::query_as("SELECT coalesce(sum(account_id=?),0),count(*) FROM download_jobs WHERE status IN ('queued','running')")
            .bind(account).fetch_one(&mut *connection).await?;
        if account_pending >= 32 || pending >= 256 {
            return Ok(Admission::Busy);
        }
        let origin = match self
            .plugins
            .capture_download_origin_on(connection, account, recording)
            .await
        {
            Ok(origin) => origin,
            Err(Error::Conflict(_)) => return Ok(Admission::WaitingForSource),
            Err(error) => return Err(error),
        };
        let intent = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO download_intents(id,account_id,recording_id,library_id,access_grant_id,host_device,state,
            provider_source_id,binding_id,binding_revision,remote_source_id,installation_id,installation_revision,service_id,service_revision,credential_digest,audio_generation)
            VALUES(?,?,?,?,?,?,'queued',?,?,?,?,?,?,?,?,?,?)")
            .bind(&intent).bind(account).bind(recording).bind(library).bind(grant).bind(self.host.as_str())
            .bind(origin.provider_source_id).bind(origin.binding_id).bind(origin.binding_revision).bind(origin.remote_source_id)
            .bind(origin.installation_id).bind(origin.installation_revision).bind(origin.service_id).bind(origin.service_revision)
            .bind(origin.credential_digest).bind(origin.audio_generation).execute(&mut *connection).await?;
        sqlx::query("INSERT INTO download_jobs(id,account_id,intent_id,generation,status) VALUES(?,?,?,1,'queued')")
            .bind(Uuid::new_v4().to_string()).bind(account).bind(&intent).execute(connection).await?;
        Ok(Admission::Retained(intent))
    }

    pub(super) async fn retain_existing_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        recording: &str,
        grant: &str,
    ) -> Result<Option<String>> {
        let existing: Option<(String, String, String, bool, bool)> = sqlx::query_as("SELECT i.id,i.access_grant_id,i.state,
            EXISTS(SELECT 1 FROM download_files f WHERE f.id=i.file_id AND f.state='ready' AND f.account_id=i.account_id
                AND f.recording_id=i.recording_id AND f.host_device=i.host_device) AS reusable,
            (NOT EXISTS(SELECT 1 FROM recording_source_choices choice WHERE choice.user_id=i.account_id
                AND choice.recording_id=i.recording_id AND choice.binding_id IS NOT NULL)
                OR (EXISTS(SELECT 1 FROM source_choice_bindings permitted WHERE permitted.user_id=i.account_id
                    AND permitted.recording_id=i.recording_id AND permitted.binding_id=i.binding_id)
                    AND (i.state!='ready' OR EXISTS(SELECT 1 FROM download_file_origins origin WHERE origin.file_id=i.file_id
                        AND origin.account_id=i.account_id AND origin.recording_id=i.recording_id
                        AND origin.host_device=i.host_device AND origin.binding_id=i.binding_id)))) AS eligible
            FROM download_intents i WHERE i.account_id=? AND i.recording_id=? AND i.host_device=? AND i.state!='removed'")
            .bind(account).bind(recording).bind(self.host.as_str()).fetch_optional(&mut *connection).await?;
        let Some((previous, previous_grant, state, reusable, eligible)) = existing else {
            return Ok(None);
        };
        if eligible && previous_grant == grant && state != "failed" {
            return Ok(Some(previous));
        }
        sqlx::query("UPDATE download_intents SET state='removed',generation=generation+1,updated_at=unixepoch() WHERE id=?")
            .bind(&previous).execute(&mut *connection).await?;
        sqlx::query("DELETE FROM download_retirement_work WHERE transfer_id=?")
            .bind(&previous)
            .execute(&mut *connection)
            .await?;
        sqlx::query("UPDATE download_jobs SET status='cancelled',updated_at=unixepoch() WHERE intent_id=? AND status='queued'")
            .bind(&previous).execute(&mut *connection).await?;
        if state == "ready" && reusable && eligible {
            let intent = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO download_intents(id,account_id,recording_id,library_id,access_grant_id,host_device,state,file_id,
                provider_source_id,binding_id,binding_revision,remote_source_id,installation_id,installation_revision,service_id,service_revision,
                credential_digest,audio_generation,bytes_received,total_bytes)
                SELECT ?,account_id,recording_id,library_id,?,host_device,'ready',file_id,
                provider_source_id,binding_id,binding_revision,remote_source_id,installation_id,installation_revision,service_id,service_revision,
                credential_digest,audio_generation,bytes_received,total_bytes FROM download_intents WHERE id=?")
                .bind(&intent).bind(grant).bind(&previous).execute(&mut *connection).await?;
            tracing::debug!(intent_id = %intent, outcome = "retained_after_access_change", "Owned download retained with current access");
            return Ok(Some(intent));
        }
        sqlx::query("UPDATE download_files SET state='deleting',cleanup_after=0 WHERE id=(SELECT file_id FROM download_intents WHERE id=?)
            AND state IN ('staging','ready') AND NOT EXISTS(SELECT 1 FROM download_intents reference WHERE reference.file_id=download_files.id AND reference.state!='removed')")
            .bind(previous).execute(&mut *connection).await?;
        Ok(None)
    }
}
