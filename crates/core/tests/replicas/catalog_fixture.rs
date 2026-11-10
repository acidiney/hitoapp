use super::fixture::REMOTE_ACCOUNT;
use hito_core::BeginCatalogSnapshot;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn snapshot(id: Uuid, ids: &[&str]) -> (Value, Vec<Value>) {
    let metadata = json!({"id":id,"account_id":REMOTE_ACCOUNT,"library_id":"remote-library","mode":"snapshot_only","collection_watermark":491,"recording_count":ids.len(),"page_count":ids.len().max(1),"expires_at":1});
    let mut pages=ids.iter().enumerate().map(|(ordinal, recording)| json!({"snapshot_id":id,"ordinal":ordinal,"next_ordinal":(ordinal+1<ids.len()).then_some(ordinal+1),"recordings":[{"id":recording,"library_id":"remote-library","title":format!("Track {ordinal}"),"artist":"Artist","duration_ms":1000,"sources":[{"id":format!("source-{ordinal}"),"observation_id":Some("legacy-observation"),"title":"Source title","artist":"Artist","duration_ms":999,"server_relay":"ready","release":{"id":"album","entry_id":format!("entry-{ordinal}"),"title":"Album","artist":null,"disc_number":1,"track_number":ordinal+1}}]}]})).collect::<Vec<_>>();
    if pages.is_empty() {
        pages.push(json!({"snapshot_id":id,"ordinal":0,"next_ordinal":null,"recordings":[]}));
    }
    (metadata, pages)
}

pub async fn install(
    services: &hito_core::Services,
    actor: &hito_core::Actor,
    session: &hito_core::ReplicaSession,
) -> hito_core::Result<hito_core::CatalogSnapshot> {
    services
        .replicas
        .install_catalog(
            actor,
            session,
            BeginCatalogSnapshot {
                library_id: "remote-library".into(),
            },
        )
        .await
}
