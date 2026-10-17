use super::Downloads;
use crate::Result;
use sqlx::SqliteConnection;
use uuid::Uuid;

impl Downloads {
    pub(super) async fn retain_existing_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        recording: &str,
        grant: &str,
    ) -> Result<Option<String>> {
        let existing: Option<(String, String, String, bool)> = sqlx::query_as("SELECT i.id,i.access_grant_id,i.state,
            EXISTS(SELECT 1 FROM download_files f WHERE f.id=i.file_id AND f.state='ready' AND f.account_id=i.account_id
                AND f.recording_id=i.recording_id AND f.host_device=i.host_device) AS reusable
            FROM download_intents i WHERE i.account_id=? AND i.recording_id=? AND i.host_device=? AND i.state!='removed'")
            .bind(account).bind(recording).bind(self.host.as_str()).fetch_optional(&mut *connection).await?;
        let Some((previous, previous_grant, state, reusable)) = existing else {
            return Ok(None);
        };
        if previous_grant == grant && state != "failed" {
            return Ok(Some(previous));
        }
        sqlx::query("UPDATE download_intents SET state='removed',generation=generation+1,updated_at=unixepoch() WHERE id=?")
            .bind(&previous).execute(&mut *connection).await?;
        sqlx::query("UPDATE download_jobs SET status='cancelled',updated_at=unixepoch() WHERE intent_id=? AND status='queued'")
            .bind(&previous).execute(&mut *connection).await?;
        if state == "ready" && reusable {
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
            AND state!='ready' AND NOT EXISTS(SELECT 1 FROM download_intents reference WHERE reference.file_id=download_files.id AND reference.state!='removed')")
            .bind(previous).execute(&mut *connection).await?;
        Ok(None)
    }
}
