use super::{Prepared, historical, snapshot};
use futures_util::FutureExt;
use hito_core::{
    Action, ArtistIdentityChoiceReceipt, ArtistIdentitySelection, ArtistReviewContext,
    ChooseCreditMemberIdentity, ChooseRecordingArtistIdentity, CoreConfig, CreditRole, Error,
    HostDeviceId, MetadataProvider, Operation, RecordingArtistIdentityState, Services,
    test_support::{
        artist_catalog_discovery_service as catalog, database_owner::DatabaseOwner,
        hang_guard::hang_guard,
    },
};
use hito_plugin_sdk::{Capability, Manifest, PROTOCOL_VERSION};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, migrate::Migrator};
use std::{
    borrow::Cow,
    panic::{AssertUnwindSafe, resume_unwind},
    path::Path,
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

const OWNER: &str = "00000000-0000-4000-8000-000000000010";
const RECORDING: &str = "00000000-0000-4000-8000-000000000101";
const WHOLE: &str = "00000000-0000-4000-8000-000000000102";
const GROUP: &str = "00000000-0000-4000-8000-000000000103";
const MAIN: &str = "00000000-0000-4000-8000-000000000104";
const FEATURED: &str = "00000000-0000-4000-8000-000000000105";
const PEER: &str = "00000000-0000-4000-8000-000000000090";
const PEER_SESSION: &str = "owned-member-upgrade-peer-session";
const INSTALLATION: &str = "00000000-0000-4000-8000-000000000061";
const MAIN_ARTIST: &str = "owned-upgrade-ada";
const FEATURED_ARTIST: &str = "owned-upgrade-cy";

type ReceiptRows = Vec<(i64, String, String, String, String)>;

struct Seeded {
    rows: Vec<snapshot::Projection>,
    receipts: ReceiptRows,
    keys: Vec<(i64, String, String, String)>,
    history: historical::History,
    whole_request: ChooseRecordingArtistIdentity,
    whole_receipt: Value,
}

#[tokio::test]
async fn opening_schema_96_keeps_credit_targets_and_accepts_private_member_choices() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let database = directory.path().join("member-upgrade.db");
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        database.clone(),
        music,
        HostDeviceId::new("member-upgrade97").unwrap(),
    );
    owner.attach(&mut config);
    let mut provider = None;
    let mut connection = None;
    let mut services = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        provider = Some(
            hang_guard(
                "the owned upgrade catalog must start",
                Box::pin(catalog::DiscoveryServer::start()),
            )
            .await,
        );
        let seeded = prepare96(&database, provider.as_ref().unwrap()).await;
        tokio::time::timeout(Duration::from_secs(30), async {
            connection = Some(super::connect(&database).await);
            let inspector = connection.as_mut().unwrap();
            hito_core::test_support::migrations()
                .run(&mut *inspector)
                .await
                .unwrap();
            snapshot::assert_preserved(inspector, &seeded.rows).await;
            assert_eq!(association_keys(inspector).await, seeded.keys);
            assert_history(inspector, 98).await;
            assert_eq!(member_count(inspector).await, 0);
            assert_database(inspector).await;
            connection.take().unwrap().close().await.unwrap();
            services = Some(hito_core::open(config).await.unwrap());
            connection = Some(super::connect(&database).await);
            snapshot::assert_preserved(connection.as_mut().unwrap(), &seeded.rows).await;
            assert_eq!(
                association_keys(connection.as_mut().unwrap()).await,
                seeded.keys
            );
            assert_eq!(member_count(connection.as_mut().unwrap()).await, 0);
            connection.take().unwrap().close().await.unwrap();
            exercise(
                services.as_ref().unwrap(),
                &database,
                seeded,
                provider.as_ref().unwrap(),
                &mut connection,
            )
            .await;
        })
        .await
        .expect("The reopened v96-to97 public workflow must settle within its existing deadline");
    }))
    .catch_unwind()
    .await;
    if let Some(connection) = connection.take() {
        connection.close().await.unwrap();
    }
    let closed = if let Some(hito) = services.take() {
        let playback = hito.playback.close().await;
        let replicas = hito.replicas.close().await;
        let replication = hito.catalog_replication.close().await;
        drop(hito);
        [playback.is_ok(), replicas.is_ok(), replication.is_ok()]
    } else {
        [true; 3]
    };
    let joined = if let Some(provider) = provider.as_mut() {
        provider.finish().await
    } else {
        true
    };
    let retired = owner.retire(Duration::from_secs(5)).await;
    assert!(closed.into_iter().all(|closed| closed) && joined);
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    assert_eq!(owner.observe().active_resources(), 0);
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

async fn prepare96(database: &Path, provider: &catalog::DiscoveryServer) -> Seeded {
    let Prepared { history, .. } = super::prepare(database).await;
    let mut connection = None;
    let outcome = AssertUnwindSafe(Box::pin(hang_guard(
        "the valid populated v96 database must be prepared",
        Box::pin(async {
        connection = Some(super::connect(database).await);
        let connection = connection.as_mut().unwrap();
        Migrator {
            migrations: Cow::Owned(hito_core::test_support::migrations().iter()
                .filter(|migration| migration.version <= 96).cloned().collect()),
            ..Migrator::DEFAULT
        }.run(&mut *connection).await.unwrap();
        assert_history(connection, 96).await;
        let absent: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name='recording_artist_member_identity_choices'")
            .fetch_one(&mut *connection).await.unwrap();
        assert_eq!(absent, 0);
        let mut tx = connection.begin().await.unwrap();
        sqlx::query("UPDATE plugin_services SET base_url=? WHERE id='00000000-0000-4000-8000-000000000060'")
            .bind(&provider.url).execute(&mut *tx).await.unwrap();
        let manifest = Manifest { id: "org.hito.artist-discovery-fixture".into(), name: "Owned artist discovery".into(),
            protocol_version: PROTOCOL_VERSION, capabilities: [Capability::Catalog].into(), authorization: None, content_policy: None };
        sqlx::query("UPDATE plugin_installations SET manifest=? WHERE id=?")
            .bind(serde_json::to_string(&manifest).unwrap()).bind(INSTALLATION).execute(&mut *tx).await.unwrap();
        let providers = vec![MetadataProvider::BuiltinMusicBrainz, member_provider()];
        sqlx::query("UPDATE account_metadata_policies SET providers=? WHERE user_id=?")
            .bind(serde_json::to_string(&providers).unwrap()).bind(OWNER).execute(&mut *tx).await.unwrap();
        sqlx::raw_sql(
            "INSERT INTO recordings(id,library_id,title,artist,duration_ms) VALUES
                ('00000000-0000-4000-8000-000000000101','owned-upgrade-library','Owned member upgrade duet','Ada & Bryn feat. Cy',100000);
             INSERT INTO sources(id,recording_id,provider,provider_id,ready) VALUES
                ('owned-member-upgrade-source','00000000-0000-4000-8000-000000000101','local','owned-member-upgrade-source',0);
             INSERT INTO metadata_observations(id,source_id,recording_id,provider,origin,inspection_version,title,artist,duration_ms) VALUES
                ('owned-member-upgrade-observation','owned-member-upgrade-source','00000000-0000-4000-8000-000000000101','local','file_inspection',1,'Owned member upgrade duet','Ada & Bryn feat. Cy',100000);
             INSERT INTO source_metadata(source_id,title,artist,duration_ms,observation_id) VALUES
                ('owned-member-upgrade-source','Owned member upgrade duet','Ada & Bryn feat. Cy',100000,'owned-member-upgrade-observation');
             UPDATE recordings SET metadata_source_id='owned-member-upgrade-source',metadata_observation_id='owned-member-upgrade-observation'
                WHERE id='00000000-0000-4000-8000-000000000101';
             INSERT INTO recording_credit_observations(observation_id,recording_id,credit_label,credit_key) VALUES
                ('owned-member-upgrade-observation','00000000-0000-4000-8000-000000000101','Ada & Bryn','ada'||char(31)||'bryn');
             INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role) VALUES
                ('owned-member-upgrade-observation',0,'00000000-0000-4000-8000-000000000101','Ada','main'),
                ('owned-member-upgrade-observation',1,'00000000-0000-4000-8000-000000000101','Bryn','main'),
                ('owned-member-upgrade-observation',2,'00000000-0000-4000-8000-000000000101','Cy','featured');
             INSERT INTO account_artists(user_id,id,library_id,name) VALUES
                ('00000000-0000-4000-8000-000000000010','owned-upgrade-combined','owned-upgrade-library','Ada & Bryn'),
                ('00000000-0000-4000-8000-000000000010','owned-upgrade-bryn','owned-upgrade-library','Bryn'),
                ('00000000-0000-4000-8000-000000000010','owned-upgrade-cy','owned-upgrade-library','Cy');
             INSERT INTO account_credit_keys(user_id,library_id,credit_key,artist_id) VALUES
                ('00000000-0000-4000-8000-000000000010','owned-upgrade-library','ada'||char(31)||'bryn','owned-upgrade-combined');
             INSERT INTO account_recording_credits(user_id,recording_id,artist_id) VALUES
                ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000101','owned-upgrade-combined');
             INSERT INTO account_recording_credit_members(user_id,recording_id,artist_id,role) VALUES
                ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000101','owned-upgrade-ada','main'),
                ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000101','owned-upgrade-bryn','main'),
                ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000101','owned-upgrade-cy','featured');
             INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,source_id,observation_id,role,provider_kind,provider_artist_id,evidence,credit_target) VALUES
                ('00000000-0000-4000-8000-000000000102','00000000-0000-4000-8000-000000000010','owned-upgrade-library','00000000-0000-4000-8000-000000000101','owned-upgrade-combined','owned-member-upgrade-source','owned-member-upgrade-observation','recording_artist','builtin_musicbrainz','00000000-0000-4000-8000-000000000022','user_confirmed','whole_credit');
             INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,provider_artist_id) VALUES
                ('00000000-0000-4000-8000-000000000103','00000000-0000-4000-8000-000000000010','builtin_musicbrainz','00000000-0000-4000-8000-000000000022');
             INSERT INTO users(id,username,password_hash) VALUES
                ('00000000-0000-4000-8000-000000000090','owned-member-upgrade-peer','owned-unused-peer-hash');"
        ).execute(&mut *tx).await.unwrap();
        for (association, artist, identity, target) in [
            (MAIN, MAIN_ARTIST, catalog::FIRST_ARTIST, "main_member"),
            (FEATURED, FEATURED_ARTIST, catalog::SECOND_ARTIST, "featured_member"),
        ] {
            sqlx::query("INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,source_id,observation_id,role,provider_kind,plugin_installation_id,provider_artist_id,evidence,credit_target)
                VALUES(?,?,'owned-upgrade-library',?,?,'owned-member-upgrade-source','owned-member-upgrade-observation','recording_artist','http_plugin',?,?,'user_confirmed',?)")
                .bind(association).bind(OWNER).bind(RECORDING).bind(artist).bind(INSTALLATION).bind(identity).bind(target)
                .execute(&mut *tx).await.unwrap();
        }
        for (revision, association) in [(1_i64, WHOLE), (2, MAIN), (3, FEATURED)] {
            sqlx::query("INSERT INTO recording_artist_association_changes(user_id,recording_id,revision,association_id,linked) VALUES(?,?,?,?,1)")
                .bind(OWNER).bind(RECORDING).bind(revision).bind(association).execute(&mut *tx).await.unwrap();
        }
        let operation = Operation { id: Uuid::from_u128(202), action: Action::RecordingTitleSet {
            recording_id: Uuid::parse_str(RECORDING).unwrap(), title: "Private member upgrade title".into() } };
        sqlx::query("INSERT INTO sync_events(user_id,origin,device_id,event_id,payload) VALUES(?,'client_operation','owned-member-upgrade',?,?)")
            .bind(OWNER).bind(operation.id.to_string()).bind(serde_json::to_string(&operation).unwrap()).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO personal_recording_metadata(user_id,recording_id,override_title,title,artist,title_operation_id)
            VALUES(?,?,'Private member upgrade title','Private member upgrade title','Ada & Bryn feat. Cy',?)")
            .bind(OWNER).bind(RECORDING).bind(operation.id.to_string()).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO sessions(token_hash,user_id,expires_at,created_at,last_seen_at) VALUES(?,?,4102444800,unixepoch(),unixepoch())")
            .bind(format!("{:x}",Sha256::digest(PEER_SESSION.as_bytes()))).bind(PEER).execute(&mut *tx).await.unwrap();
        let grant: String = sqlx::query_scalar("SELECT id FROM library_access_grants WHERE library_id='owned-upgrade-library' AND user_id=?")
            .bind(OWNER).fetch_one(&mut *tx).await.unwrap();
        let whole_request = ChooseRecordingArtistIdentity {
            request_id: Uuid::from_u128(201), expected_revision: 0, association_id: Uuid::parse_str(WHOLE).unwrap(),
            context: ArtistReviewContext { account_id: OWNER.into(), recording_id: RECORDING.into(), library_id: "owned-upgrade-library".into(),
                artist_id: "owned-upgrade-combined".into(), credited_name: "Ada & Bryn".into(),
                source_id: Some("owned-member-upgrade-source".into()), observation_id: "owned-member-upgrade-observation".into(),
                artist_operation_id: None, access_grant_id: grant, metadata_binding: None,
                provider: MetadataProvider::BuiltinMusicBrainz, policy_revision: 1, connection: None },
        };
        let whole_receipt = ArtistIdentityChoiceReceipt { request_id: whole_request.request_id,
            recording_id: RECORDING.into(), revision: 1, selection: Some(ArtistIdentitySelection { group_id: GROUP.into(), association_id: WHOLE.into() }) };
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&("hito-artist-identity-choose:v1",RECORDING,&whole_request)).unwrap()));
        sqlx::query("INSERT INTO artist_identity_choice_receipts(acceptance_order,user_id,request_id,request_digest,receipt) VALUES(704,?,?,?,?)")
            .bind(OWNER).bind(whole_request.request_id.to_string()).bind(digest).bind(serde_json::to_string(&whole_receipt).unwrap()).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,observation_id,selection_order)
            VALUES(?,?,1,?,?,'owned-upgrade-combined','owned-member-upgrade-observation',704)")
            .bind(OWNER).bind(RECORDING).bind(WHOLE).bind(GROUP).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert_database(connection).await;
        let targets: Vec<String> = sqlx::query_scalar("SELECT credit_target FROM recording_artist_associations WHERE recording_id=? ORDER BY credit_target")
            .bind(RECORDING).fetch_all(&mut *connection).await.unwrap();
        assert_eq!(targets,["featured_member","main_member","whole_credit"]);
        let receipts = old_receipts(connection).await;
        assert_eq!(receipts.len(),4);
        let keys = association_keys(connection).await;
        assert_eq!(keys.len(),6);
        let rows = snapshot::capture(connection,96).await;
        Seeded { rows, receipts, keys, history, whole_request, whole_receipt: serde_json::to_value(whole_receipt).unwrap() }
    })))).catch_unwind().await;
    if let Some(connection) = connection.take() {
        connection.close().await.unwrap();
    }
    match outcome {
        Ok(seeded) => seeded,
        Err(panic) => resume_unwind(panic),
    }
}

async fn exercise(
    hito: &Services,
    database: &Path,
    seeded: Seeded,
    provider: &catalog::DiscoveryServer,
    connection: &mut Option<SqliteConnection>,
) {
    let actor = hito.identity.authenticate(super::SESSION).await.unwrap();
    let peer = hito.identity.authenticate(PEER_SESSION).await.unwrap();
    let old_choice = serde_json::to_value(
        hito.artist_catalog
            .recording_artist_identity_choice(&actor, super::RECORDING)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(old_choice["revision"], 3);
    let whole = serde_json::to_value(
        hito.artist_catalog
            .recording_artist_identity_choice(&actor, RECORDING)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(whole["revision"], 1);
    assert_eq!(whole["state"]["association_id"], WHOLE);
    let metadata = serde_json::to_value(
        hito.catalog
            .recording_metadata(&actor, RECORDING, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        metadata["title"]["personal"],
        "Private member upgrade title"
    );
    assert!(metadata["artist"]["personal"].is_null());
    assert_eq!(metadata["credited_artists"].as_array().unwrap().len(), 3);
    let associations = serde_json::to_value(
        hito.artist_catalog
            .recording_artist_associations(&actor, RECORDING)
            .await
            .unwrap(),
    )
    .unwrap();
    let changes = serde_json::to_value(
        hito.artist_catalog
            .recording_artist_association_changes(&actor, RECORDING)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(changes["revision"], 3);
    let requests = provider.requests().len();
    seeded.history.replay(hito, &actor).await;
    assert_eq!(
        serde_json::to_value(
            hito.artist_catalog
                .choose_recording_artist_identity(&actor, RECORDING, seeded.whole_request)
                .await
                .unwrap()
        )
        .unwrap(),
        seeded.whole_receipt
    );
    let mut selected = Vec::new();
    for (artist, association, role, identity) in [
        (MAIN_ARTIST, MAIN, CreditRole::Main, catalog::FIRST_ARTIST),
        (
            FEATURED_ARTIST,
            FEATURED,
            CreditRole::Featured,
            catalog::SECOND_ARTIST,
        ),
    ] {
        let before = hito
            .artist_catalog
            .recording_credit_member_identity_choice(&actor, RECORDING, artist)
            .await
            .unwrap();
        assert_eq!(before.revision, 0);
        assert!(matches!(
            before.state,
            RecordingArtistIdentityState::Unselected
        ));
        let context = hito
            .artist_catalog
            .begin_credit_member_review(&actor, RECORDING, artist, member_provider())
            .await
            .unwrap();
        assert_eq!(context.role, role);
        let request = json!(ChooseCreditMemberIdentity {
            request_id: Uuid::new_v4(),
            expected_revision: 0,
            association_id: Uuid::parse_str(association).unwrap(),
            context
        });
        let receipt = hito
            .artist_catalog
            .choose_credit_member_identity(
                &actor,
                RECORDING,
                artist,
                serde_json::from_value(request.clone()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(receipt.revision, 1);
        assert_eq!(receipt.role, role);
        let selection = receipt.selection.as_ref().unwrap();
        assert_eq!(selection.association_id, association);
        assert_ne!(selection.group_id, GROUP);
        let current = hito
            .artist_catalog
            .recording_credit_member_identity_choice(&actor, RECORDING, artist)
            .await
            .unwrap();
        assert!(
            matches!(&current.state,RecordingArtistIdentityState::Selected(value) if value.association_id==association && value.group_id==selection.group_id)
        );
        let detail = hito
            .artist_catalog
            .artist_identity_group(&actor, &selection.group_id, Default::default())
            .await
            .unwrap();
        assert_eq!(detail.group.provider, member_provider());
        assert_eq!(detail.group.provider_artist_id, identity);
        assert_eq!(detail.group.recording_count, 1);
        assert_eq!(detail.recordings.total, 1);
        assert_eq!(detail.recordings.items[0].recording.id, RECORDING);
        assert_eq!(detail.recordings.items[0].association_id, association);
        assert!(matches!(
            hito.artist_catalog
                .recording_credit_member_identity_choice(&peer, RECORDING, artist)
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.artist_catalog
                .choose_credit_member_identity(
                    &peer,
                    RECORDING,
                    artist,
                    serde_json::from_value(request).unwrap()
                )
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.artist_catalog
                .artist_identity_group(&peer, &selection.group_id, Default::default())
                .await,
            Err(Error::NotFound)
        ));
        selected.push(serde_json::to_value(current).unwrap());
    }
    assert_ne!(
        selected[0]["state"]["group_id"],
        selected[1]["state"]["group_id"]
    );
    assert_eq!(
        serde_json::to_value(
            hito.artist_catalog
                .recording_credit_member_identity_choice(&actor, RECORDING, MAIN_ARTIST)
                .await
                .unwrap()
        )
        .unwrap(),
        selected[0]
    );
    assert_eq!(
        hito.artist_catalog
            .artist_identity_groups(&peer, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        serde_json::to_value(
            hito.artist_catalog
                .recording_artist_identity_choice(&actor, super::RECORDING)
                .await
                .unwrap()
        )
        .unwrap(),
        old_choice
    );
    assert_eq!(
        serde_json::to_value(
            hito.artist_catalog
                .recording_artist_identity_choice(&actor, RECORDING)
                .await
                .unwrap()
        )
        .unwrap(),
        whole
    );
    assert_eq!(
        serde_json::to_value(
            hito.catalog
                .recording_metadata(&actor, RECORDING, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        metadata
    );
    assert_eq!(
        serde_json::to_value(
            hito.artist_catalog
                .recording_artist_associations(&actor, RECORDING)
                .await
                .unwrap()
        )
        .unwrap(),
        associations
    );
    assert_eq!(
        serde_json::to_value(
            hito.artist_catalog
                .recording_artist_association_changes(&actor, RECORDING)
                .await
                .unwrap()
        )
        .unwrap(),
        changes
    );
    assert_eq!(provider.requests().len(), requests);
    assert_eq!(provider.unexpected(), 0);
    *connection = Some(super::connect(database).await);
    let inspector = connection.as_mut().unwrap();
    let outcome = AssertUnwindSafe(Box::pin(async {
        assert_eq!(old_receipts(inspector).await, seeded.receipts);
        assert_eq!(association_keys(inspector).await, seeded.keys);
        assert_eq!(member_count(inspector).await, 2);
        let peer_rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM recording_artist_member_identity_choices WHERE user_id=?",
        )
        .bind(PEER)
        .fetch_one(&mut *inspector)
        .await
        .unwrap();
        assert_eq!(peer_rows, 0);
        assert_database(inspector).await;
    }))
    .catch_unwind()
    .await;
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

fn member_provider() -> MetadataProvider {
    MetadataProvider::HttpPlugin {
        installation_id: INSTALLATION.into(),
    }
}

async fn old_receipts(connection: &mut SqliteConnection) -> ReceiptRows {
    sqlx::query_as("SELECT acceptance_order,user_id,request_id,request_digest,receipt FROM artist_identity_choice_receipts WHERE acceptance_order<=704 ORDER BY acceptance_order")
        .fetch_all(connection).await.unwrap()
}

async fn association_keys(connection: &mut SqliteConnection) -> Vec<(i64, String, String, String)> {
    sqlx::query_as("SELECT rowid,id,provider_key,credit_target FROM recording_artist_associations ORDER BY rowid")
        .fetch_all(connection).await.unwrap()
}

async fn member_count(connection: &mut SqliteConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM recording_artist_member_identity_choices")
        .fetch_one(connection)
        .await
        .unwrap()
}

async fn assert_history(connection: &mut SqliteConnection, latest: i64) {
    let history: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success=1 ORDER BY version")
            .fetch_all(connection)
            .await
            .unwrap();
    assert_eq!(history, (1..=latest).collect::<Vec<_>>());
}

async fn assert_database(connection: &mut SqliteConnection) {
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        1
    );
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(connection)
        .await
        .unwrap();
    assert_eq!(integrity, ["ok"]);
}
