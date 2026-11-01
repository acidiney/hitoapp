#[path = "source_preparation_history.rs"]
mod provider;

use super::support;
use hito_core::{AdoptProviderRecording, InstallPlugin, OfferPluginService, PrepareSource};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use uuid::Uuid;

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub account: String,
    pub recording: String,
    pub source: String,
    pub preparation: String,
    pub service: String,
}

impl Fixture {
    pub async fn start() -> Self {
        let provider = provider::Provider::start().await;
        let (directory, hito, actor, _, _) = support::library("preparation-query-plans").await;
        let service = hito
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Owned query plan fixture".into(),
                    base_url: provider.url.clone(),
                },
            )
            .await
            .unwrap();
        let installation = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id.clone(),
                    service_revision: service.revision,
                    bearer_token: Some(provider::TOKEN.into()),
                },
            )
            .await
            .unwrap();
        let recording = hito
            .catalog
            .adopt_provider(
                &actor,
                &installation.id,
                AdoptProviderRecording {
                    installation_revision: installation.revision,
                    provider_recording_id: provider::RECORDING.into(),
                },
            )
            .await
            .unwrap();
        let source = hito
            .catalog
            .sources(&actor, &recording.id)
            .await
            .unwrap()
            .remove(0);
        let receipt = hito
            .catalog
            .prepare_source(
                &actor,
                &recording.id,
                &source.id,
                PrepareSource {
                    request_id: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();
        assert_eq!(provider.work_counts(), (0, 0, 0, 0));
        Self {
            directory,
            account: actor.user().id.clone(),
            recording: recording.id,
            source: source.id,
            preparation: receipt.preparation_id.to_string(),
            service: service.id,
        }
    }

    pub async fn connection(&self) -> SqliteConnection {
        SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(self.directory.path().join("hito.db"))
                .foreign_keys(false),
        )
        .await
        .unwrap()
    }

    pub async fn history(
        &self,
        connection: &mut SqliteConnection,
        preparations: i64,
        receipts: i64,
    ) {
        sqlx::query("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < ?)
            INSERT OR IGNORE INTO source_preparations(id,user_id,recording_id,source_id,library_id,access_grant,
                binding_id,binding_revision,provider_recording_id,remote_source_id,installation_id,installation_revision,
                service_id,service_revision,provider,audio_generation,phase,submission_state,remote_outcome,created_at,next_check_at)
            SELECT 'history-' || n.i,p.user_id,p.recording_id,p.source_id,p.library_id,p.access_grant,
                p.binding_id,p.binding_revision,p.provider_recording_id,p.remote_source_id,p.installation_id,p.installation_revision,
                p.service_id,p.service_revision,p.provider,p.audio_generation,'ready','submitted','ready',p.created_at,NULL
            FROM n CROSS JOIN source_preparations p WHERE p.id = ?")
            .bind(preparations).bind(&self.preparation).execute(&mut *connection).await.unwrap();
        sqlx::query("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < ?)
            INSERT OR IGNORE INTO source_preparation_receipts(user_id,request_id,request_digest,preparation_id)
            SELECT p.user_id,printf('00000000-0000-4000-8000-%012x',n.i),printf('%064d',0),p.id
            FROM n CROSS JOIN source_preparations p WHERE p.id = ?")
            .bind(receipts).bind(&self.preparation).execute(&mut *connection).await.unwrap();
        let counts: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM source_preparations),
            (SELECT count(*) FROM source_preparation_receipts)",
        )
        .fetch_one(connection)
        .await
        .unwrap();
        assert_eq!(counts, (preparations + 3, receipts + 1));
    }

    pub async fn active(&self, connection: &mut SqliteConnection) {
        sqlx::query("WITH n(i) AS (VALUES(0),(1))
            INSERT INTO source_preparations(id,user_id,recording_id,source_id,library_id,access_grant,
                binding_id,binding_revision,provider_recording_id,remote_source_id,installation_id,installation_revision,
                service_id,service_revision,provider,audio_generation,phase,submission_state,remote_handle,
                lease_token,lease_until,created_at,next_check_at)
            SELECT CASE n.i WHEN 0 THEN 'query-expired' ELSE 'query-stale' END,'query-account-' || n.i,
                p.recording_id,'query-source-' || n.i,p.library_id,p.access_grant,
                p.binding_id,p.binding_revision,p.provider_recording_id,'query-source-' || n.i,p.installation_id,p.installation_revision,
                p.service_id,p.service_revision,p.provider,p.audio_generation,
                CASE n.i WHEN 0 THEN 'preparing' ELSE 'queued' END,
                CASE n.i WHEN 0 THEN 'submitted' ELSE 'unsubmitted' END,
                CASE n.i WHEN 0 THEN 'query-handle' END,CASE n.i WHEN 0 THEN 'query-lease' END,
                CASE n.i WHEN 0 THEN 0 END,2000,NULL
            FROM n CROSS JOIN source_preparations p WHERE p.id = ?")
            .bind(&self.preparation).execute(connection).await.unwrap();
    }

    pub async fn retire_active(connection: &mut SqliteConnection) {
        sqlx::query("UPDATE source_preparations SET phase = CASE submission_state WHEN 'submitted' THEN 'failed' ELSE 'cancelled' END,
            remote_outcome = CASE submission_state WHEN 'submitted' THEN 'failed' END,
            authority_cancelled = 1,next_check_at = NULL,lease_token = NULL,lease_until = NULL
            WHERE remote_outcome IS NULL")
            .execute(connection).await.unwrap();
    }

    pub async fn exceed_pending_limits(&self, connection: &mut SqliteConnection) {
        let inserted = sqlx::query("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < 256)
            INSERT INTO source_preparations(id,user_id,recording_id,source_id,library_id,access_grant,
                binding_id,binding_revision,provider_recording_id,remote_source_id,installation_id,installation_revision,
                service_id,service_revision,provider,audio_generation)
            SELECT 'cap-' || n.i,CASE WHEN n.i <= 32 THEN p.user_id ELSE 'cap-account-' || (n.i / 32) END,
                p.recording_id,'cap-source-' || n.i,p.library_id,p.access_grant,
                p.binding_id,p.binding_revision,p.provider_recording_id,'cap-source-' || n.i,p.installation_id,p.installation_revision,
                p.service_id,p.service_revision,p.provider,p.audio_generation
            FROM n CROSS JOIN source_preparations p WHERE p.id = ?")
            .bind(&self.preparation).execute(connection).await.unwrap();
        assert_eq!(inserted.rows_affected(), 256);
    }
}
