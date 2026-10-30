use crate::{
    Actor, ChooseRecordingArtistIdentity, CoreConfig, Credentials, HostDeviceId, MetadataProvider,
    Services,
};
use futures_util::TryStreamExt;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::path::PathBuf;
use uuid::Uuid;

pub(super) struct Fixture {
    directory: tempfile::TempDir,
    music: PathBuf,
    pub(super) token: String,
    pub(super) recording: String,
    pub(super) association: String,
    pub(super) artist: String,
    pub(super) observation: String,
}

impl Fixture {
    pub(super) fn config(&self) -> CoreConfig {
        CoreConfig::new(
            self.directory.path().join("hito.db"),
            self.music.clone(),
            HostDeviceId::new("identity-retirement-work").unwrap(),
        )
    }

    pub(super) async fn prepare() -> (Self, Services, Actor) {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let mut fixture = Self {
            directory,
            music,
            token: String::new(),
            recording: String::new(),
            association: String::new(),
            artist: String::new(),
            observation: String::new(),
        };
        let hito = crate::open(fixture.config()).await.unwrap();
        let session = hito
            .identity
            .setup(Credentials {
                username: "identity-work-owner".into(),
                password: "owned identity work password".into(),
            })
            .await
            .unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        fixture.token = session.token;
        write_wav(&fixture.music.join("owned.wav"));
        let library = hito.catalog.libraries(&actor).await.unwrap().remove(0).id;
        let job = hito
            .storage
            .enqueue_scan(&actor, &library)
            .await
            .unwrap()
            .remove(0);
        assert!(hito.storage.run_next_job().await.unwrap());
        assert_eq!(
            hito.storage.job(&actor, &job.id).await.unwrap().state,
            "complete"
        );
        fixture.recording = hito
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .items
            .remove(0)
            .id;
        let context = hito
            .artist_catalog
            .begin_artist_review(
                &actor,
                &fixture.recording,
                MetadataProvider::BuiltinMusicBrainz,
            )
            .await
            .unwrap();
        fixture.artist = context.credited_name.clone();
        fixture.observation = context.observation_id.clone();
        let association = hito
            .artist_catalog
            .confirm_artist_review(&actor, &context, "00000000-0000-4000-8000-000000000001")
            .await
            .unwrap();
        fixture.association = association.id.clone();
        hito.artist_catalog
            .choose_recording_artist_identity(
                &actor,
                &fixture.recording,
                ChooseRecordingArtistIdentity {
                    request_id: Uuid::new_v4(),
                    expected_revision: 0,
                    association_id: Uuid::parse_str(&association.id).unwrap(),
                    context,
                },
            )
            .await
            .unwrap();
        (fixture, hito, actor)
    }

    pub(super) async fn select(&self, hito: &Services, actor: &Actor) {
        let current = hito
            .artist_catalog
            .recording_artist_identity_choice(actor, &self.recording)
            .await
            .unwrap();
        let context = hito
            .artist_catalog
            .begin_artist_review(actor, &self.recording, MetadataProvider::BuiltinMusicBrainz)
            .await
            .unwrap();
        hito.artist_catalog
            .choose_recording_artist_identity(
                actor,
                &self.recording,
                ChooseRecordingArtistIdentity {
                    request_id: Uuid::new_v4(),
                    expected_revision: current.revision,
                    association_id: Uuid::parse_str(&self.association).unwrap(),
                    context,
                },
            )
            .await
            .unwrap();
    }
}

pub(super) async fn grow(pool: &SqlitePool, template: &str, start: i64, end: i64) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("CREATE TEMP TABLE identity_work_fixture AS WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i+1<?)
        SELECT printf('a5100000-0000-4000-8000-%012x',i) AS recording,printf('a5200000-0000-4000-8000-%012x',i) AS source,
        printf('a5300000-0000-4000-8000-%012x',i) AS observation,printf('a5400000-0000-4000-8000-%012x',i) AS association,
        printf('a5500000-0000-4000-8000-%012x',i) AS receipt FROM n")
        .bind(start).bind(end).execute(&mut *tx).await.unwrap();
    for sql in [
        "INSERT INTO recordings(id,library_id,release_id,title,artist,duration_ms,provenance)
         SELECT f.recording,r.library_id,r.release_id,r.title,r.artist,r.duration_ms,r.provenance FROM identity_work_fixture f CROSS JOIN recordings r WHERE r.id=?",
        "INSERT INTO sources(id,recording_id,provider,provider_id,device_id,ready)
         SELECT f.source,f.recording,s.provider,s.provider_id,s.device_id,s.ready FROM identity_work_fixture f CROSS JOIN recordings r JOIN sources s ON s.id=r.metadata_source_id WHERE r.id=?",
        "INSERT INTO metadata_observations(id,source_id,recording_id,release_id,provider,origin,observed_at,file_checksum,inspection_version,title,artist,album,album_artist,duration_ms,tags)
         SELECT f.observation,f.source,f.recording,o.release_id,o.provider,o.origin,o.observed_at,o.file_checksum,o.inspection_version,o.title,o.artist,o.album,o.album_artist,o.duration_ms,o.tags
         FROM identity_work_fixture f CROSS JOIN recordings r JOIN metadata_observations o ON o.id=r.metadata_observation_id WHERE r.id=?",
        "INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,disc_number,track_number,inspection_version,observation_id)
         SELECT f.source,m.release_id,m.title,m.artist,m.duration_ms,m.disc_number,m.track_number,m.inspection_version,f.observation
         FROM identity_work_fixture f CROSS JOIN recordings r JOIN source_metadata m ON m.source_id=r.metadata_source_id WHERE r.id=?",
        "INSERT INTO account_recording_credits(user_id,recording_id,artist_id)
         SELECT credit.user_id,f.recording,credit.artist_id FROM identity_work_fixture f CROSS JOIN account_recording_credits credit WHERE credit.recording_id=?",
        "INSERT INTO account_source_credits(user_id,source_id,recording_id,release_id,artist_id)
         SELECT credit.user_id,f.source,f.recording,credit.release_id,credit.artist_id FROM identity_work_fixture f CROSS JOIN account_source_credits credit WHERE credit.recording_id=?",
    ] { sqlx::query(sql).bind(template).execute(&mut *tx).await.unwrap(); }
    sqlx::query("UPDATE recordings AS r SET metadata_source_id=f.source,metadata_observation_id=f.observation FROM identity_work_fixture f WHERE r.id=f.recording")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,source_id,observation_id,role,provider_kind,plugin_installation_id,provider_artist_id,evidence)
        SELECT f.association,a.user_id,a.library_id,f.recording,a.artist_id,f.source,f.observation,a.role,a.provider_kind,a.plugin_installation_id,a.provider_artist_id,a.evidence
        FROM identity_work_fixture f CROSS JOIN recording_artist_associations a WHERE a.recording_id=?")
        .bind(template).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO artist_identity_choice_receipts(user_id,request_id,request_digest,receipt)
        SELECT c.user_id,f.receipt,'owned-work-fixture',json_set(ack.receipt,'$.request_id',f.receipt,'$.recording_id',f.recording,'$.selection.association_id',f.association)
        FROM identity_work_fixture f CROSS JOIN recording_artist_identity_choices c
        JOIN artist_identity_choice_receipts ack ON ack.acceptance_order=c.selection_order WHERE c.recording_id=?")
        .bind(template).execute(&mut *tx).await.unwrap();
    let inserted=sqlx::query("INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,artist_operation_id,observation_id,retired,selected_at,selection_order)
        SELECT c.user_id,f.recording,c.revision,f.association,c.group_id,c.artist_id,c.artist_operation_id,f.observation,c.retired,c.selected_at,ack.acceptance_order
        FROM identity_work_fixture f CROSS JOIN recording_artist_identity_choices c
        JOIN artist_identity_choice_receipts ack ON ack.user_id=c.user_id AND ack.request_id=f.receipt WHERE c.recording_id=?")
        .bind(template).execute(&mut *tx).await.unwrap();
    assert_eq!(inserted.rows_affected(), (end - start) as u64);
    sqlx::query("DROP TABLE identity_work_fixture")
        .execute(&mut *tx)
        .await
        .unwrap();
    let failures: Vec<(String, Option<i64>, String, i64)> =
        sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert!(failures.is_empty());
    tx.commit().await.unwrap();
}

pub(super) async fn unrelated_digest(pool: &SqlitePool, target: &str) -> String {
    let mut digest = Sha256::new();
    let mut rows=sqlx::query_as::<_,(String,String,i64,Option<String>,Option<String>,Option<String>,Option<String>,Option<String>,i64,i64,Option<i64>)>(
        "SELECT user_id,recording_id,revision,association_id,group_id,artist_id,artist_operation_id,observation_id,retired,selected_at,selection_order
         FROM recording_artist_identity_choices WHERE recording_id!=? ORDER BY user_id,recording_id").bind(target).fetch(pool);
    while let Some(row) = rows.try_next().await.unwrap() {
        digest.update(serde_json::to_vec(&row).unwrap());
    }
    format!("{:x}", digest.finalize())
}

fn write_wav(path: &std::path::Path) {
    let samples = 8000_u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&16000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(samples * 2).to_le_bytes());
    for sample in 0..samples {
        bytes.extend_from_slice(&((sample % 100) as i16 * 100).to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}
