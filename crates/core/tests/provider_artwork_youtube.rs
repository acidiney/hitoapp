#[allow(unused_imports)]
#[path = "fixtures/provider_attachment.rs"]
mod fixture;
use hito_core::test_support::support;

use hito_core::AdoptProviderRecording;
use serde_json::json;

async fn availability(capabilities: &[&str]) -> bool {
    let youtube = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) =
        support::library("youtube-artwork-availability").await;
    let installation =
        fixture::connect(&hito, &actor, &youtube, "org.hito.youtube", capabilities).await;
    youtube
        .reply(
            "v1/recordings/detail",
            json!({"id": "HitoTone001", "title": "Owned tone", "artist": "Channel: Hito"}),
        )
        .await;
    youtube
        .reply(
            "v1/sources",
            json!({"items": [{"id": "youtube:v1:HitoTone001:313430", "label": "AAC / M4A", "ready": true}]}),
        )
        .await;
    let saved = hito
        .catalog
        .adopt_provider(
            &actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: "HitoTone001".into(),
            },
        )
        .await
        .unwrap();
    hito.catalog
        .artwork_availability(&actor, &saved.id)
        .await
        .unwrap()
        .can_fetch
}

#[tokio::test]
async fn saved_youtube_recording_can_fetch_artwork_only_when_the_manifest_declares_it() {
    assert!(
        availability(&["search", "metadata", "sources", "artwork"]).await,
        "a YouTube-shaped manifest with artwork must make the saved recording fetchable"
    );
    assert!(
        !availability(&["search", "metadata", "sources"]).await,
        "a connection approved before the capability stays unavailable"
    );
}
